use crate::{
    engine::{Counts, Report},
    reports::{self, Snapshot},
    scope::Scope,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub source: String,
    pub backup: String,
    pub started: u64,
    pub finished: u64,
    pub summary: Report,
    pub scope: Scope,
    pub mode: String,
    pub error: Option<String>,
}

pub fn data_dir() -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("MIRRORPROOF_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    std::env::var_os("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("Crelyn Studio").join("MirrorProof"))
        .ok_or_else(|| "storageFailed".into())
}

pub fn id() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string()
}
pub fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 40 && value.bytes().all(|b| b.is_ascii_digit())
}

pub fn safe_dir(path: &Path) -> Result<Vec<std::fs::File>, String> {
    reports::local_path(path)?;
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("storageFailed".into());
    }
    let mut current = PathBuf::new();
    let mut guards = Vec::new();
    for part in path.components() {
        current.push(part.as_os_str());
        if matches!(part, std::path::Component::Normal(_)) {
            if !current.exists() {
                fs::create_dir(&current).map_err(|_| "storageFailed")?;
            }
            guards.extend(
                crate::engine::pin_report_directory(&current).map_err(|_| "storageFailed")?,
            );
        }
    }
    Ok(guards)
}

fn outside(base: &Path, roots: &[PathBuf]) -> Result<(), String> {
    let mut existing = base;
    while !existing.exists() {
        existing = existing.parent().ok_or("storageFailed")?;
    }
    let _pins = crate::engine::pin_report_directory(existing)?;
    let resolved = fs::canonicalize(existing).map_err(|_| "storageFailed")?;
    if roots
        .iter()
        .any(|root| reports::inside(base, root) || reports::inside(&resolved, root))
    {
        return Err("historyInsideScan".into());
    }
    Ok(())
}

pub fn save_at(base: &Path, snapshot: &Snapshot) -> Result<String, String> {
    outside(base, &snapshot.roots)?;
    let folder = base.join("history");
    let _guards = safe_dir(&folder)?;
    let id = id();
    reports::write_new(snapshot, &folder.join(format!("{id}.json")), "json")?;
    let entry = Entry {
        id: id.clone(),
        source: snapshot.source.clone(),
        backup: snapshot.backup.clone(),
        started: snapshot.started_at_unix_seconds,
        finished: snapshot.finished_at_unix_seconds,
        summary: snapshot.summary.clone(),
        scope: snapshot.scope.clone(),
        mode: snapshot.mode.clone(),
        error: None,
    };
    reports::write_document(
        &snapshot.roots,
        &folder.join(format!("{id}.meta.json")),
        "json",
        &serde_json::to_vec(&entry).map_err(|_| "storageFailed")?,
    )?;
    Ok(id)
}

pub fn save(snapshot: &Snapshot) -> Result<String, String> {
    save_at(&data_dir()?, snapshot)
}

pub fn failed(source: &str, backup: &str, scope: &Scope, error: &str) {
    let Ok(base) = data_dir() else {
        return;
    };
    let roots = [PathBuf::from(source), PathBuf::from(backup)];
    if outside(&base, &roots).is_err() {
        return;
    }
    let folder = base.join("history");
    let Ok(_guards) = safe_dir(&folder) else {
        return;
    };
    let id = id();
    let entry = Entry {
        id: id.clone(),
        source: source.into(),
        backup: backup.into(),
        started: reports::now(),
        finished: reports::now(),
        summary: Report {
            cancelled: false,
            processed: 0,
            total: 0,
            bytes_read: 0,
            duration_ms: 0,
            counts: Counts::default(),
        },
        scope: scope.clone(),
        mode: "folders".into(),
        error: Some(error.into()),
    };
    if let Ok(bytes) = serde_json::to_vec(&entry) {
        let _ = reports::write_document(
            &roots,
            &folder.join(format!("{id}.meta.json")),
            "json",
            &bytes,
        );
    }
}

pub fn list_at(base: &Path) -> Result<Vec<Entry>, String> {
    let folder = base.join("history");
    if !folder.exists() {
        return Ok(Vec::new());
    }
    let _guards = crate::engine::pin_report_directory(&folder)?;
    let mut entries = Vec::new();
    for item in fs::read_dir(&folder).map_err(|_| "storageFailed")? {
        let item = item.map_err(|_| "storageFailed")?;
        let name = item.file_name();
        let name = name.to_string_lossy();
        if let Some(id) = name.strip_suffix(".meta.json").filter(|id| valid_id(id)) {
            if fs::symlink_metadata(item.path())
                .map_err(|_| "storageFailed")?
                .len()
                > 65536
            {
                continue;
            }
            let bytes = reports::read_document(&item.path())?;
            let entry: Entry = serde_json::from_slice(&bytes).map_err(|_| "storageFailed")?;
            if entry.id != id {
                return Err("storageFailed".into());
            }
            entries.push(entry);
        }
    }
    entries.sort_by(|a, b| b.finished.cmp(&a.finished).then_with(|| b.id.cmp(&a.id)));
    entries.truncate(100);
    Ok(entries)
}

pub fn load_at(base: &Path, id: &str) -> Result<Snapshot, String> {
    if !valid_id(id) {
        return Err("invalidDocument".into());
    }
    reports::decode(&reports::read_document(
        &base.join("history").join(format!("{id}.json")),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    #[test]
    fn history_round_trip_and_compared_root_protection() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a");
        let b = temp.path().join("b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        fs::write(a.join("one"), "same").unwrap();
        fs::write(b.join("one"), "same").unwrap();
        let mut rows = Vec::new();
        let summary = crate::engine::compare(
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            &AtomicBool::new(false),
            |p| {
                rows.extend(p.rows);
                true
            },
        )
        .unwrap();
        let scan = Snapshot {
            source: a.to_string_lossy().into_owned(),
            backup: b.to_string_lossy().into_owned(),
            started_at_unix_seconds: 1,
            finished_at_unix_seconds: 2,
            summary,
            rows,
            scope: Scope::default(),
            mode: "folders".into(),
            baseline_complete: None,
            roots: vec![fs::canonicalize(&a).unwrap(), fs::canonicalize(&b).unwrap()],
        };
        let storage = temp.path().join("storage");
        let id = save_at(&storage, &scan).unwrap();
        assert_eq!(list_at(&storage).unwrap().len(), 1);
        assert_eq!(load_at(&storage, &id).unwrap().summary.counts.matched, 1);
        assert!(load_at(&storage, "../a").is_err());
        assert_eq!(
            save_at(&a.join("history-data"), &scan).unwrap_err(),
            "historyInsideScan"
        );
        assert!(!a.join("history-data").exists());
    }
}
