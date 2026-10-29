use crate::scope::Scope;
use serde::{Deserialize, Serialize};
pub mod manifest;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Read},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Instant, SystemTime},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Matched,
    Different,
    Missing,
    Extra,
    Error,
    Skipped,
    Unverified,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub path: String,
    pub status: Status,
    pub reason: &'static str,
    pub side: &'static str,
    pub source_size: Option<u64>,
    pub backup_size: Option<u64>,
    pub os_code: Option<i32>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Counts {
    pub matched: u64,
    pub different: u64,
    pub missing: u64,
    pub extra: u64,
    pub error: u64,
    pub skipped: u64,
    pub unverified: u64,
}

impl Counts {
    pub(crate) fn add(&mut self, status: Status) {
        *match status {
            Status::Matched => &mut self.matched,
            Status::Different => &mut self.different,
            Status::Missing => &mut self.missing,
            Status::Extra => &mut self.extra,
            Status::Error => &mut self.error,
            Status::Skipped => &mut self.skipped,
            Status::Unverified => &mut self.unverified,
        } += 1;
    }
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: &'static str,
    pub indexed: u64,
    pub processed: u64,
    pub total: u64,
    pub bytes_read: u64,
    pub total_bytes: u64,
    pub current_path: String,
    pub counts: Counts,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub cancelled: bool,
    pub processed: u64,
    pub total: u64,
    pub bytes_read: u64,
    pub duration_ms: u64,
    pub counts: Counts,
}

impl<'de> Deserialize<'de> for Row {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            path: String,
            status: Status,
            reason: String,
            side: String,
            source_size: Option<u64>,
            backup_size: Option<u64>,
            os_code: Option<i32>,
        }
        let v = Wire::deserialize(d)?;
        let reason = [
            "sourceOnly",
            "backupOnly",
            "sha256Match",
            "hashMismatch",
            "sizeMismatch",
            "typeMismatch",
            "reparsePoint",
            "readError",
            "changedDuringScan",
            "directoryChanged",
            "incompleteEnumeration",
            "depthLimit",
            "unsupportedFile",
            "excludedByRule",
            "incompleteManifest",
            "hashRecorded",
        ]
        .into_iter()
        .find(|s| *s == v.reason)
        .ok_or_else(|| serde::de::Error::custom("Unknown result reason"))?;
        let side = ["source", "backup", "both"]
            .into_iter()
            .find(|s| *s == v.side)
            .ok_or_else(|| serde::de::Error::custom("Unknown result side"))?;
        Ok(Self {
            path: v.path,
            status: v.status,
            reason,
            side,
            source_size: v.source_size,
            backup_size: v.backup_size,
            os_code: v.os_code,
        })
    }
}

#[derive(Clone)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
}

impl Stamp {
    fn new(m: &Metadata) -> Self {
        Self {
            len: m.len(),
            modified: m.modified().ok(),
            created: m.created().ok(),
        }
    }
    fn unchanged(&self, m: &Metadata) -> bool {
        m.is_file()
            && !is_reparse(m)
            && self.len == m.len()
            && self.modified == m.modified().ok()
            && self.created == m.created().ok()
    }
}

#[derive(Clone, Debug)]
struct Problem {
    reason: &'static str,
    status: Status,
    os_code: Option<i32>,
}

impl Problem {
    fn io(error: io::Error) -> Self {
        Self {
            reason: "readError",
            status: Status::Error,
            os_code: error.raw_os_error(),
        }
    }
    fn new(reason: &'static str, status: Status) -> Self {
        Self {
            reason,
            status,
            os_code: None,
        }
    }
}

enum Entry {
    File(Stamp),
    Directory,
    Blocked(Problem),
}

#[derive(Default)]
struct Inventory {
    entries: BTreeMap<PathBuf, Entry>,
    directories: Vec<(PathBuf, BTreeSet<std::ffi::OsString>)>,
    blocked: BTreeSet<PathBuf>,
}

struct Context<'a, F: FnMut(Progress) -> bool> {
    cancel: &'a AtomicBool,
    sink: F,
    progress: Progress,
    last_emit: Instant,
}

impl<F: FnMut(Progress) -> bool> Context<'_, F> {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    fn emit(&mut self, force: bool) {
        if force || self.progress.rows.len() >= 128 || self.last_emit.elapsed().as_millis() >= 100 {
            let mut message = self.progress.clone();
            message.rows = std::mem::take(&mut self.progress.rows);
            if !(self.sink)(message) {
                self.cancel.store(true, Ordering::Release);
            }
            self.last_emit = Instant::now();
        }
    }

    fn row(&mut self, row: Row) {
        self.progress.counts.add(row.status);
        self.progress.rows.push(row);
        self.emit(false);
    }
}

#[cfg(windows)]
fn is_reparse(m: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_reparse(m: &Metadata) -> bool {
    m.file_type().is_symlink()
}

fn open_directory(path: &Path) -> Result<File, Problem> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Exclude writable/deletable directory handles while inspecting each ancestor.
        options
            .access_mode(0x81)
            .share_mode(1)
            .custom_flags(0x02200000);
    }
    let file = options.open(path).map_err(Problem::io)?;
    let metadata = file.metadata().map_err(Problem::io)?;
    if is_reparse(&metadata) {
        return Err(Problem::new("reparsePoint", Status::Skipped));
    }
    if !metadata.is_dir() {
        return Err(Problem::new("changedDuringScan", Status::Error));
    }
    Ok(file)
}

fn pin_ancestors(path: &Path) -> Result<Vec<File>, Problem> {
    let mut current = PathBuf::new();
    let mut handles = Vec::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Normal(_)) {
            handles.push(open_directory(&current)?);
        }
    }
    Ok(handles)
}

pub(crate) fn pin_report_directory(path: &Path) -> Result<Vec<File>, String> {
    pin_ancestors(path).map_err(|_| "exportFolderUnsafe".to_string())
}

#[cfg(windows)]
pub(crate) fn local_drive(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDriveTypeW(root: *const u16) -> u32;
    }
    let mut root = PathBuf::new();
    for component in path.components().take(2) {
        root.push(component);
    }
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    matches!(unsafe { GetDriveTypeW(wide.as_ptr()) }, 2 | 3 | 6)
}

#[cfg(not(windows))]
pub(crate) fn local_drive(_: &Path) -> bool {
    true
}

fn checked_root(input: &str) -> Result<(PathBuf, Vec<File>), String> {
    let path = Path::new(input);
    crate::reports::local_path(path).map_err(|_| "localFoldersOnly")?;
    let mut guards = pin_ancestors(path).map_err(|p| p.reason.to_string())?;
    let normalized = fs::canonicalize(path).map_err(|_| "invalidFolder")?;
    guards.push(open_directory(&normalized).map_err(|p| p.reason.to_string())?);
    Ok((normalized, guards))
}

pub(crate) fn roots(source: &str, backup: &str) -> Result<(PathBuf, PathBuf, Vec<File>), String> {
    let mut guards = Vec::new();
    let mut checked = Vec::new();
    for input in [source, backup] {
        let (normalized, root_guards) = checked_root(input)?;
        guards.extend(root_guards);
        checked.push(normalized);
    }
    let a = checked.remove(0);
    let b = checked.remove(0);
    // Root overlap is conservative even on case-sensitive Windows directories.
    let folded_a = a.to_string_lossy().to_lowercase();
    let folded_b = b.to_string_lossy().to_lowercase();
    if Path::new(&folded_a).starts_with(Path::new(&folded_b))
        || Path::new(&folded_b).starts_with(Path::new(&folded_a))
    {
        return Err("overlappingFolders".into());
    }
    Ok((a, b, guards))
}

fn check_inventory<F: FnMut(Progress) -> bool>(
    root: &Path,
    inventory: &Inventory,
    side: &'static str,
    ctx: &mut Context<F>,
) -> Vec<Row> {
    let mut issues = Vec::new();
    for (relative, names) in &inventory.directories {
        if ctx.cancelled() {
            break;
        }
        ctx.progress.current_path = relative.to_string_lossy().into_owned();
        let current = pin_ancestors(&root.join(relative)).and_then(|_guards| {
            let entries = fs::read_dir(root.join(relative)).map_err(Problem::io)?;
            let mut current_names = BTreeSet::new();
            for entry in entries {
                if ctx.cancelled() {
                    break;
                }
                current_names.insert(entry.map_err(Problem::io)?.file_name());
            }
            Ok(current_names)
        });
        if !ctx.cancelled() && current.as_ref().ok() != Some(names) {
            let mut row = base_row(relative, None, None);
            row.status = Status::Error;
            row.reason = "directoryChanged";
            row.side = side;
            issues.push(row.clone());
            ctx.row(row);
        }
        ctx.emit(false);
    }
    issues
}

fn block(inventory: &mut Inventory, relative: &Path, problem: Problem) {
    inventory.blocked.insert(relative.to_owned());
    inventory
        .entries
        .insert(relative.to_owned(), Entry::Blocked(problem));
}

fn walk<F: FnMut(Progress) -> bool>(
    root: &Path,
    relative: &Path,
    depth: usize,
    inventory: &mut Inventory,
    ctx: &mut Context<F>,
    scope: &Scope,
) {
    if ctx.cancelled() {
        return;
    }
    if depth > 128 {
        block(
            inventory,
            relative,
            Problem::new("depthLimit", Status::Skipped),
        );
        return;
    }
    let path = root.join(relative);
    let _directory = match open_directory(&path) {
        Ok(handle) => handle,
        Err(problem) => {
            block(inventory, relative, problem);
            return;
        }
    };
    let mut names = BTreeSet::new();
    let entries = match fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(error) => {
            block(inventory, relative, Problem::io(error));
            return;
        }
    };
    for next in entries {
        if ctx.cancelled() {
            break;
        }
        let item = match next {
            Ok(item) => item,
            Err(error) => {
                block(inventory, relative, Problem::io(error));
                continue;
            }
        };
        let child = relative.join(item.file_name());
        names.insert(item.file_name());
        ctx.progress.indexed += 1;
        ctx.progress.current_path = child.to_string_lossy().into_owned();
        ctx.emit(false);
        if scope.excludes(&child.to_string_lossy()) {
            block(
                inventory,
                &child,
                Problem::new("excludedByRule", Status::Skipped),
            );
            continue;
        }
        let metadata = match fs::symlink_metadata(item.path()) {
            Ok(metadata) => metadata,
            Err(error) => {
                block(inventory, &child, Problem::io(error));
                continue;
            }
        };
        if is_reparse(&metadata) {
            block(
                inventory,
                &child,
                Problem::new("reparsePoint", Status::Skipped),
            );
        } else if metadata.is_dir() {
            inventory.entries.insert(child.clone(), Entry::Directory);
            walk(root, &child, depth + 1, inventory, ctx, scope);
        } else if metadata.is_file() {
            inventory
                .entries
                .insert(child, Entry::File(Stamp::new(&metadata)));
        } else {
            block(
                inventory,
                &child,
                Problem::new("unsupportedFile", Status::Skipped),
            );
        }
    }
    inventory.directories.push((relative.to_owned(), names));
}

fn under_blocked(inventory: &Inventory, relative: &Path) -> bool {
    relative.ancestors().any(|p| inventory.blocked.contains(p))
}

fn size(entry: Option<&Entry>) -> Option<u64> {
    match entry {
        Some(Entry::File(stamp)) => Some(stamp.len),
        _ => None,
    }
}

fn base_row(relative: &Path, a: Option<&Entry>, b: Option<&Entry>) -> Row {
    Row {
        path: if relative.as_os_str().is_empty() {
            ".".into()
        } else {
            relative.to_string_lossy().into_owned()
        },
        status: Status::Unverified,
        reason: "incompleteEnumeration",
        side: "both",
        source_size: size(a),
        backup_size: size(b),
        os_code: None,
    }
}

fn checked_file(root: &Path, relative: &Path, stamp: &Stamp) -> Result<(File, Vec<File>), Problem> {
    let path = root.join(relative);
    let guards = pin_ancestors(
        path.parent()
            .ok_or_else(|| Problem::new("readError", Status::Error))?,
    )?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Read access only; exclude writers/deleters while hashing, and never follow a final link.
        options.share_mode(1).custom_flags(0x08200000);
    }
    let file = options.open(path).map_err(Problem::io)?;
    let metadata = file.metadata().map_err(Problem::io)?;
    if !stamp.unchanged(&metadata) {
        return Err(Problem::new("changedDuringScan", Status::Error));
    }
    Ok((file, guards))
}

fn hash<F: FnMut(Progress) -> bool>(
    file: &mut File,
    ctx: &mut Context<F>,
) -> Result<Option<[u8; 32]>, Problem> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        if ctx.cancelled() {
            return Ok(None);
        }
        let count = file.read(&mut buffer).map_err(Problem::io)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        ctx.progress.bytes_read = ctx.progress.bytes_read.saturating_add(count as u64);
        ctx.emit(false);
    }
    Ok(Some(hasher.finalize().into()))
}

fn compare_files<F: FnMut(Progress) -> bool>(
    source: &Path,
    backup: &Path,
    relative: &Path,
    a: &Stamp,
    b: &Stamp,
    ctx: &mut Context<F>,
) -> Result<Option<(Status, &'static str)>, (&'static str, Problem)> {
    let (mut left, _left_guards) = checked_file(source, relative, a).map_err(|p| ("source", p))?;
    let (mut right, _right_guards) =
        checked_file(backup, relative, b).map_err(|p| ("backup", p))?;
    if a.len != b.len {
        return Ok(Some((Status::Different, "sizeMismatch")));
    }
    let left_hash = hash(&mut left, ctx).map_err(|p| ("source", p))?;
    let right_hash = hash(&mut right, ctx).map_err(|p| ("backup", p))?;
    if left_hash.is_none() || right_hash.is_none() {
        return Ok(None);
    }
    for (file, stamp, side) in [(&left, a, "source"), (&right, b, "backup")] {
        if !stamp.unchanged(&file.metadata().map_err(|e| (side, Problem::io(e)))?) {
            return Err((side, Problem::new("changedDuringScan", Status::Error)));
        }
    }
    Ok(Some(if left_hash == right_hash {
        (Status::Matched, "sha256Match")
    } else {
        (Status::Different, "hashMismatch")
    }))
}

#[cfg(test)]
pub fn compare<F: FnMut(Progress) -> bool>(
    source: &str,
    backup: &str,
    cancel: &AtomicBool,
    sink: F,
) -> Result<Report, String> {
    compare_scoped(source, backup, cancel, sink, &Scope::default())
}

pub fn compare_scoped<F: FnMut(Progress) -> bool>(
    source: &str,
    backup: &str,
    cancel: &AtomicBool,
    sink: F,
    scope: &Scope,
) -> Result<Report, String> {
    scope.validate()?;
    let started = Instant::now();
    let (source, backup, _root_guards) = roots(source, backup)?;
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
    let mut left = Inventory::default();
    let mut right = Inventory::default();
    walk(&source, Path::new(""), 0, &mut left, &mut ctx, scope);
    walk(&backup, Path::new(""), 0, &mut right, &mut ctx, scope);
    let paths: BTreeSet<_> = left
        .entries
        .keys()
        .chain(right.entries.keys())
        .cloned()
        .collect();
    let paths: Vec<_> = paths
        .into_iter()
        .filter(|p| {
            let a = left.entries.get(p);
            let b = right.entries.get(p);
            !matches!(
                (a, b),
                (Some(Entry::Directory), Some(Entry::Directory) | None)
                    | (None, Some(Entry::Directory))
            )
        })
        .collect();
    ctx.progress.total = paths.len() as u64;
    ctx.progress.total_bytes = paths.iter().fold(0u64, |total, p| {
        match (left.entries.get(p), right.entries.get(p)) {
            (Some(Entry::File(a)), Some(Entry::File(b))) if a.len == b.len => {
                total.saturating_add(a.len.saturating_mul(2))
            }
            _ => total,
        }
    });
    ctx.progress.phase = "hashing";
    ctx.emit(true);
    for relative in paths {
        if ctx.cancelled() {
            break;
        }
        let a = left.entries.get(&relative);
        let b = right.entries.get(&relative);
        ctx.progress.current_path = relative.to_string_lossy().into_owned();
        let mut row = base_row(&relative, a, b);
        match (a, b) {
            (Some(Entry::Blocked(problem)), _) | (_, Some(Entry::Blocked(problem))) => {
                row.status = problem.status;
                row.reason = problem.reason;
                row.os_code = problem.os_code;
                row.side = if matches!(a, Some(Entry::Blocked(_))) {
                    "source"
                } else {
                    "backup"
                };
            }
            (Some(Entry::File(a)), Some(Entry::File(b))) => {
                match compare_files(&source, &backup, &relative, a, b, &mut ctx) {
                    Ok(Some((status, reason))) => {
                        row.status = status;
                        row.reason = reason;
                    }
                    Ok(None) => break,
                    Err((side, problem)) => {
                        row.status = problem.status;
                        row.reason = problem.reason;
                        row.os_code = problem.os_code;
                        row.side = side;
                    }
                }
            }
            (Some(_), None) if !under_blocked(&right, &relative) => {
                row.status = Status::Missing;
                row.reason = "sourceOnly";
                row.side = "backup";
            }
            (None, Some(_)) if !under_blocked(&left, &relative) => {
                row.status = Status::Extra;
                row.reason = "backupOnly";
                row.side = "backup";
            }
            (Some(Entry::Directory), Some(Entry::File(_)))
            | (Some(Entry::File(_)), Some(Entry::Directory)) => {
                row.status = Status::Different;
                row.reason = "typeMismatch";
            }
            _ => {}
        }
        ctx.progress.processed += 1;
        ctx.row(row);
    }
    if !ctx.cancelled() {
        ctx.progress.phase = "checking";
        ctx.emit(true);
        for (root, inventory, side) in [(&source, &left, "source"), (&backup, &right, "backup")] {
            let _ = check_inventory(root, inventory, side, &mut ctx);
        }
    }
    ctx.progress.phase = if ctx.cancelled() { "cancelled" } else { "done" };
    ctx.progress.current_path.clear();
    ctx.emit(true);
    Ok(Report {
        cancelled: ctx.cancelled(),
        processed: ctx.progress.processed,
        total: ctx.progress.total,
        bytes_read: ctx.progress.bytes_read,
        duration_ms: started.elapsed().as_millis() as u64,
        counts: ctx.progress.counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn exclusions_are_real_skipped_results_and_do_not_enter_excluded_subtrees() {
        let (_temp, a, b) = fixture();
        for root in [&a, &b] {
            fs::write(root.join("keep"), "same").unwrap();
            fs::create_dir(root.join("cache")).unwrap();
            fs::write(root.join("cache/secret"), "ignored").unwrap();
            fs::write(root.join("skip.tmp"), "ignored").unwrap();
        }
        let scope = Scope {
            exclusions: vec!["cache/".into(), "*.tmp".into()],
        };
        let mut rows = Vec::new();
        let report = compare_scoped(
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            &AtomicBool::new(false),
            |p| {
                rows.extend(p.rows);
                true
            },
            &scope,
        )
        .unwrap();
        assert_eq!(report.counts.matched, 1);
        assert_eq!(report.counts.skipped, 2);
        assert_eq!(report.bytes_read, 8);
        assert!(!rows.iter().any(|r| r.path.contains("secret")));
        assert!(rows
            .iter()
            .filter(|r| r.status == Status::Skipped)
            .all(|r| r.reason == "excludedByRule"));
    }

    fn fixture() -> (TempDir, PathBuf, PathBuf) {
        let temp = TempDir::new().unwrap();
        let a = temp.path().join("source");
        let b = temp.path().join("backup");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        (temp, a, b)
    }

    fn run(a: &Path, b: &Path) -> (Report, Vec<Row>) {
        let mut rows = Vec::new();
        let report = compare(
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            &AtomicBool::new(false),
            |p| {
                rows.extend(p.rows);
                true
            },
        )
        .unwrap();
        (report, rows)
    }

    #[test]
    fn compares_real_content_including_equal_size_changes_and_empty_files() {
        let (_temp, a, b) = fixture();
        for root in [&a, &b] {
            fs::create_dir(root.join("nested")).unwrap();
            fs::write(root.join("nested/eşleşen.txt"), "same").unwrap();
            fs::write(root.join("empty"), "").unwrap();
        }
        fs::write(a.join("changed"), "ABCD").unwrap();
        fs::write(b.join("changed"), "WXYZ").unwrap();
        fs::write(a.join("sized"), "small").unwrap();
        fs::write(b.join("sized"), "longer").unwrap();
        fs::write(a.join("missing"), "source").unwrap();
        fs::write(b.join("extra"), "backup").unwrap();
        let (report, rows) = run(&a, &b);
        assert_eq!(report.counts.matched, 2);
        assert_eq!(report.counts.different, 2);
        assert_eq!(report.counts.missing, 1);
        assert_eq!(report.counts.extra, 1);
        assert_eq!(
            rows.iter().find(|r| r.path == "changed").unwrap().reason,
            "hashMismatch"
        );
        assert_eq!(fs::read(a.join("changed")).unwrap(), b"ABCD");
    }

    #[test]
    fn rejects_same_nested_and_invalid_roots() {
        let (_temp, a, b) = fixture();
        fs::create_dir(a.join("child")).unwrap();
        let cancel = AtomicBool::new(false);
        for target in [&a, &a.join("child")] {
            assert_eq!(
                compare(
                    a.to_str().unwrap(),
                    target.to_str().unwrap(),
                    &cancel,
                    |_| true
                )
                .unwrap_err(),
                "overlappingFolders"
            );
        }
        assert!(compare("relative", b.to_str().unwrap(), &cancel, |_| true).is_err());
    }

    #[test]
    fn cancellation_keeps_only_finished_results() {
        let (_temp, a, b) = fixture();
        for root in [&a, &b] {
            for index in 0..1000 {
                fs::write(root.join(format!("file-{index:04}")), "done").unwrap();
            }
        }
        let cancel = AtomicBool::new(false);
        let mut rows = Vec::new();
        let report = compare(a.to_str().unwrap(), b.to_str().unwrap(), &cancel, |p| {
            if p.processed >= 1 {
                cancel.store(true, Ordering::Release);
            }
            rows.extend(p.rows);
            true
        })
        .unwrap();
        assert!(report.cancelled);
        assert!(report.counts.matched > 0 && report.counts.matched < 1000);
        assert_eq!(rows.len() as u64, report.counts.matched);
        let cancel = AtomicBool::new(true);
        let report = compare(a.to_str().unwrap(), b.to_str().unwrap(), &cancel, |_| true).unwrap();
        assert!(report.cancelled);
        assert_eq!(report.counts.matched, 0);
    }

    #[test]
    fn cancellation_is_checked_between_hash_chunks() {
        let (_temp, a, _) = fixture();
        fs::write(a.join("large"), vec![42; 4 * 1024 * 1024]).unwrap();
        let mut file = File::open(a.join("large")).unwrap();
        let cancel = AtomicBool::new(false);
        let mut ctx = Context {
            cancel: &cancel,
            sink: |p: Progress| {
                if p.bytes_read > 0 {
                    cancel.store(true, Ordering::Release);
                }
                true
            },
            progress: Progress::default(),
            last_emit: Instant::now() - std::time::Duration::from_secs(1),
        };
        assert!(hash(&mut file, &mut ctx).unwrap().is_none());
        assert_eq!(ctx.progress.bytes_read, 1024 * 1024);
    }

    #[test]
    fn changed_file_is_rejected() {
        let (_temp, a, _) = fixture();
        let path = a.join("changed");
        fs::write(&path, "before").unwrap();
        let stamp = Stamp::new(&fs::metadata(&path).unwrap());
        fs::write(&path, "after with a different length").unwrap();
        assert_eq!(
            checked_file(&a, Path::new("changed"), &stamp)
                .unwrap_err()
                .reason,
            "changedDuringScan"
        );
    }

    #[test]
    fn added_files_invalidate_the_directory_scan() {
        let (_temp, a, b) = fixture();
        let cancel = AtomicBool::new(false);
        let mut added = false;
        let report = compare(a.to_str().unwrap(), b.to_str().unwrap(), &cancel, |p| {
            if p.phase == "hashing" && !added {
                fs::write(b.join("added-after-indexing"), "new").unwrap();
                added = true;
            }
            true
        })
        .unwrap();
        assert!(report.counts.error >= 1);
    }

    #[test]
    #[ignore = "creates 200,000 real files; run explicitly for the scale check"]
    fn hundred_thousand_files() {
        let (_temp, a, b) = fixture();
        for root in [&a, &b] {
            for directory in 0..100 {
                let folder = root.join(format!("batch-{directory:03}"));
                fs::create_dir(&folder).unwrap();
                for index in 0..1000 {
                    fs::write(folder.join(format!("file-{index:04}")), "test content").unwrap();
                }
            }
        }
        let (report, rows) = run(&a, &b);
        assert!(!report.cancelled);
        assert_eq!(report.counts.matched, 100_000);
        assert_eq!(report.counts.error, 0);
        assert_eq!(rows.len(), 100_000);
        println!(
            "100,000 file pairs verified in {} ms; {} bytes read",
            report.duration_ms, report.bytes_read
        );
    }

    #[test]
    fn never_calls_a_blocked_subtree_missing() {
        let mut inventory = Inventory::default();
        block(
            &mut inventory,
            Path::new("blocked"),
            Problem::new("readError", Status::Error),
        );
        assert!(under_blocked(&inventory, Path::new("blocked/sub/file")));
        assert!(!under_blocked(&inventory, Path::new("blocked-other/file")));
    }

    #[test]
    fn detects_file_directory_conflicts_and_empty_trees() {
        let (_temp, a, b) = fixture();
        assert_eq!(run(&a, &b).0.processed, 0);
        fs::write(a.join("conflict"), "file").unwrap();
        fs::create_dir(b.join("conflict")).unwrap();
        assert_eq!(run(&a, &b).1[0].reason, "typeMismatch");
    }

    #[cfg(windows)]
    #[test]
    fn locked_files_are_errors_and_junctions_are_not_followed() {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::process::CommandExt;
        let (_temp, a, b) = fixture();
        fs::write(a.join("locked"), "same").unwrap();
        fs::write(b.join("locked"), "same").unwrap();
        let lock = OpenOptions::new()
            .write(true)
            .share_mode(0)
            .open(a.join("locked"))
            .unwrap();
        let (report, _) = run(&a, &b);
        assert_eq!(report.counts.error, 1);
        assert_eq!(report.counts.matched, 0);
        drop(lock);
        let link = a.join("loop");
        let result = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&a)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "junction creation must succeed for this test"
        );
        let (report, rows) = run(&a, &b);
        assert_eq!(report.counts.skipped, 1);
        assert_eq!(
            rows.iter().find(|r| r.path == "loop").unwrap().reason,
            "reparsePoint"
        );
        fs::remove_dir(link).unwrap();
    }

    #[test]
    fn sha256_matches_the_known_abc_vector() {
        let (_temp, a, _) = fixture();
        fs::write(a.join("abc"), "abc").unwrap();
        let mut file = File::open(a.join("abc")).unwrap();
        let cancel = AtomicBool::new(false);
        let mut ctx = Context {
            cancel: &cancel,
            sink: |_| true,
            progress: Progress::default(),
            last_emit: Instant::now(),
        };
        let digest = hash(&mut file, &mut ctx).unwrap().unwrap();
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(
            hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
