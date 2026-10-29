use super::*;
use crate::reports::{self, Snapshot};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub application: String,
    pub schema_version: u8,
    pub algorithm: String,
    pub source: String,
    pub created: u64,
    pub complete: bool,
    pub cancelled: bool,
    pub scope: Scope,
    pub files: Vec<ManifestFile>,
    pub issues: Vec<Row>,
    pub bytes_read: u64,
    pub duration_ms: u64,
}

pub fn build<F: FnMut(Progress) -> bool>(
    folder: &str,
    scope: &Scope,
    cancel: &AtomicBool,
    sink: F,
) -> Result<Manifest, String> {
    let started = Instant::now();
    scope.validate()?;
    let (root, _guards) = checked_root(folder)?;
    let mut inventory = Inventory::default();
    let mut ctx = Context {
        cancel,
        sink,
        progress: Progress {
            phase: "indexing",
            ..Default::default()
        },
        last_emit: Instant::now(),
    };
    ctx.emit(true);
    walk(&root, Path::new(""), 0, &mut inventory, &mut ctx, scope);
    ctx.progress.total = inventory.entries.len() as u64;
    ctx.progress.total_bytes = inventory
        .entries
        .values()
        .filter_map(|e| {
            if let Entry::File(s) = e {
                Some(s.len)
            } else {
                None
            }
        })
        .fold(0, u64::saturating_add);
    ctx.progress.phase = "hashing";
    ctx.emit(true);
    let mut files = Vec::new();
    let mut issues = Vec::new();
    for (relative, entry) in &inventory.entries {
        if ctx.cancelled() {
            break;
        }
        ctx.progress.current_path = relative.to_string_lossy().into_owned();
        let mut row = base_row(relative, Some(entry), None);
        row.side = "source";
        let result = match entry {
            Entry::Directory => continue,
            Entry::Blocked(p) => Err(p.clone()),
            Entry::File(stamp) => {
                checked_file(&root, relative, stamp).and_then(|(mut file, _pins)| {
                    let value = hash(&mut file, &mut ctx)?;
                    if !stamp.unchanged(&file.metadata().map_err(Problem::io)?) {
                        return Err(Problem::new("changedDuringScan", Status::Error));
                    }
                    Ok(value)
                })
            }
        };
        match result {
            Ok(Some(digest)) => {
                files.push(ManifestFile {
                    path: row.path.clone(),
                    size: row.source_size.unwrap_or(0),
                    sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
                });
                row.status = Status::Matched;
                row.reason = "hashRecorded";
                ctx.row(row);
            }
            Ok(None) => break,
            Err(p) => {
                row.status = p.status;
                row.reason = p.reason;
                row.os_code = p.os_code;
                issues.push(row.clone());
                ctx.row(row);
            }
        }
    }
    if !ctx.cancelled() {
        ctx.progress.phase = "checking";
        ctx.emit(true);
        issues.extend(check_inventory(&root, &inventory, "source", &mut ctx));
    }
    let cancelled = ctx.cancelled();
    ctx.progress.phase = if cancelled { "cancelled" } else { "done" };
    ctx.progress.current_path.clear();
    ctx.emit(true);
    Ok(Manifest {
        application: "MirrorProof Manifest".into(),
        schema_version: 1,
        algorithm: "SHA-256".into(),
        source: folder.into(),
        created: reports::now(),
        complete: !cancelled
            && issues.is_empty()
            && scope.exclusions.is_empty()
            && !files.is_empty(),
        cancelled,
        scope: scope.clone(),
        files,
        issues,
        bytes_read: ctx.progress.bytes_read,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

pub fn decode(bytes: &[u8]) -> Result<Manifest, String> {
    let mut manifest: Manifest = serde_json::from_slice(bytes).map_err(|_| "invalidManifest")?;
    if manifest.application != "MirrorProof Manifest"
        || manifest.schema_version != 1
        || manifest.algorithm != "SHA-256"
    {
        return Err("invalidManifest".into());
    }
    manifest.scope.validate()?;
    let mut seen = BTreeSet::new();
    for file in &mut manifest.files {
        if file.path.is_empty()
            || file.path.len() > 32768
            || !reports::valid_relative(&file.path)
            || file
                .path
                .replace('\\', "/")
                .split('/')
                .any(|p| p.is_empty() || p == ".")
            || !seen.insert(file.path.replace('/', "\\"))
            || file.sha256.len() != 64
            || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("invalidManifest".into());
        }
        file.path = file.path.replace('/', "\\");
        file.sha256.make_ascii_lowercase();
    }
    if manifest
        .issues
        .iter()
        .any(|r| !reports::valid_relative(&r.path))
    {
        return Err("invalidManifest".into());
    }
    manifest.complete &= !manifest.cancelled
        && manifest.issues.is_empty()
        && manifest.scope.exclusions.is_empty()
        && !manifest.files.is_empty();
    Ok(manifest)
}

pub fn verify<F: FnMut(Progress) -> bool>(
    folder: &str,
    baseline_path: &Path,
    cancel: &AtomicBool,
    sink: F,
) -> Result<Snapshot, String> {
    let baseline = decode(&reports::read_document(baseline_path)?)?;
    let started = reports::now();
    let current = build(folder, &baseline.scope, cancel, sink)?;
    let left: BTreeMap<_, _> = current.files.iter().map(|f| (f.path.clone(), f)).collect();
    let right: BTreeMap<_, _> = baseline.files.iter().map(|f| (f.path.clone(), f)).collect();
    let paths: BTreeSet<_> = left.keys().chain(right.keys()).collect();
    let expected = paths.len() as u64;
    let mut rows = current.issues.clone();
    for path in paths {
        let a = left.get(path);
        let b = right.get(path);
        if current.cancelled && a.is_none() {
            continue;
        }
        if a.is_none() && current.issues.iter().any(|issue| issue.path == *path) {
            continue;
        }
        let (status, reason) = match (a, b) {
            (Some(a), Some(b)) => {
                if a.sha256 == b.sha256 && a.size == b.size {
                    (Status::Matched, "sha256Match")
                } else {
                    (Status::Different, "hashMismatch")
                }
            }
            (None, Some(_))
                if current.cancelled
                    || current.issues.iter().any(|i| {
                        path == &i.path
                            || i.path.is_empty()
                            || path.starts_with(&format!("{}\\", i.path))
                    }) =>
            {
                (Status::Unverified, "incompleteEnumeration")
            }
            (None, Some(_)) => (Status::Missing, "sourceOnly"),
            (Some(_), None) if !baseline.complete => (Status::Unverified, "incompleteManifest"),
            (Some(_), None) => (Status::Extra, "backupOnly"),
            _ => continue,
        };
        rows.push(Row {
            path: path.clone(),
            status,
            reason,
            side: "both",
            source_size: a.map(|f| f.size),
            backup_size: b.map(|f| f.size),
            os_code: None,
        });
    }
    if !baseline.complete {
        let mut row = base_row(Path::new(""), None, None);
        row.status = Status::Unverified;
        row.reason = "incompleteManifest";
        rows.push(row);
    }
    let mut counts = Counts::default();
    for row in &rows {
        counts.add(row.status);
    }
    let processed = rows.len() as u64;
    Ok(Snapshot {
        source: folder.into(),
        backup: baseline_path.to_string_lossy().into_owned(),
        started_at_unix_seconds: started,
        finished_at_unix_seconds: reports::now(),
        summary: Report {
            cancelled: current.cancelled,
            processed,
            total: expected.max(processed),
            bytes_read: current.bytes_read,
            duration_ms: current.duration_ms,
            counts,
        },
        rows,
        scope: baseline.scope,
        mode: "manifest".into(),
        baseline_complete: Some(baseline.complete),
        roots: vec![fs::canonicalize(folder).map_err(|_| "invalidFolder")?],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_real_hashes_detect_content_changes_and_hostile_paths() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("one"), "abc").unwrap();
        fs::write(source.join("missing"), "old").unwrap();
        let cancel = AtomicBool::new(false);
        let mut manifest = build(source.to_str().unwrap(), &Scope::default(), &cancel, |_| {
            true
        })
        .unwrap();
        assert!(manifest.complete);
        assert_eq!(
            manifest
                .files
                .iter()
                .find(|f| f.path == "one")
                .unwrap()
                .sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let path = temp.path().join("manifest.json");
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let matched = verify(source.to_str().unwrap(), &path, &cancel, |_| true).unwrap();
        assert_eq!(matched.summary.counts.matched, 2);
        fs::write(source.join("one"), "xyz").unwrap();
        fs::remove_file(source.join("missing")).unwrap();
        fs::write(source.join("extra"), "new").unwrap();
        let changed = verify(source.to_str().unwrap(), &path, &cancel, |_| true).unwrap();
        assert_eq!(changed.summary.counts.different, 1);
        assert_eq!(changed.summary.counts.missing, 1);
        assert_eq!(changed.summary.counts.extra, 1);
        manifest.files[0].path = "../escape".into();
        assert!(decode(&serde_json::to_vec(&manifest).unwrap()).is_err());
    }
    #[test]
    fn incomplete_baseline_and_cancelled_scans_never_claim_a_match() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("one"), "abc").unwrap();
        let cancel = AtomicBool::new(false);
        let mut baseline = build(source.to_str().unwrap(), &Scope::default(), &cancel, |_| {
            true
        })
        .unwrap();
        baseline.complete = false;
        let path = temp.path().join("manifest.json");
        fs::write(&path, serde_json::to_vec(&baseline).unwrap()).unwrap();
        let snapshot = verify(source.to_str().unwrap(), &path, &cancel, |_| true).unwrap();
        assert!(!reports::all_match(&snapshot));
        assert_eq!(snapshot.summary.counts.unverified, 1);
        let snapshot = verify(source.to_str().unwrap(), &path, &cancel, |p| {
            p.phase != "hashing"
        })
        .unwrap();
        assert!(snapshot.summary.cancelled);
        assert_eq!(snapshot.summary.counts.matched, 0);
        assert!(!reports::all_match(&snapshot));
    }
}
