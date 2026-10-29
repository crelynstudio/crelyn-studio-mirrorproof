use crate::engine::{pin_report_directory, Report, Row, Status};
use crate::scope::Scope;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub source: String,
    pub backup: String,
    pub started_at_unix_seconds: u64,
    pub finished_at_unix_seconds: u64,
    pub summary: Report,
    pub rows: Vec<Row>,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default = "folder_mode")]
    pub mode: String,
    #[serde(default)]
    pub baseline_complete: Option<bool>,
    #[serde(skip)]
    pub roots: Vec<PathBuf>,
}
fn folder_mode() -> String {
    "folders".into()
}

pub fn valid_relative(value: &str) -> bool {
    !value.contains(['\0', ':'])
        && !value.starts_with(['/', '\\'])
        && !value.replace('\\', "/").split('/').any(|part| part == "..")
}

pub fn read_document(path: &Path) -> Result<Vec<u8>, String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("invalidDocument".into());
    }
    local_path(path)?;
    let _guards = pin_report_directory(path.parent().ok_or("invalidDocument")?)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x00200000);
    }
    let mut file = options.open(path).map_err(|_| "invalidDocument")?;
    let metadata = file.metadata().map_err(|_| "invalidDocument")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("invalidDocument".into());
        }
    }
    if !metadata.is_file() || metadata.len() > 128 * 1024 * 1024 {
        return Err("invalidDocument".into());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(128 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "invalidDocument")?;
    if bytes.len() > 128 * 1024 * 1024 {
        return Err("invalidDocument".into());
    }
    Ok(bytes)
}

pub fn path_syntax(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("invalidDocument".into());
    }
    #[cfg(windows)]
    {
        use std::path::Prefix;
        if !matches!(path.components().next(),Some(Component::Prefix(p)) if matches!(p.kind(),Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        {
            return Err("invalidDocument".into());
        }
    }
    Ok(())
}
pub fn local_path(path: &Path) -> Result<(), String> {
    path_syntax(path)?;
    if !crate::engine::local_drive(path) {
        return Err("invalidDocument".into());
    }
    Ok(())
}

pub fn decode(bytes: &[u8]) -> Result<Snapshot, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Envelope {
        application: String,
        schema_version: u8,
        scan: Snapshot,
    }
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| "invalidDocument")?;
    if envelope.application != "MirrorProof" || envelope.schema_version != 1 {
        return Err("invalidDocument".into());
    }
    let mut scan = envelope.scan;
    scan.scope.validate()?;
    if !matches!(scan.mode.as_str(), "folders" | "manifest")
        || scan.source.len() > 32768
        || scan.backup.len() > 32768
    {
        return Err("invalidDocument".into());
    }
    for path in [&scan.source, &scan.backup] {
        path_syntax(Path::new(path))?;
        if path.contains('\0')
            || !Path::new(path).is_absolute()
            || Path::new(path)
                .components()
                .any(|p| matches!(p, Component::ParentDir))
        {
            return Err("invalidDocument".into());
        }
    }
    let mut counts = crate::engine::Counts::default();
    for row in &scan.rows {
        if row.path.len() > 32768
            || !valid_relative(&row.path)
            || (row.status == Status::Matched && row.reason != "sha256Match")
        {
            return Err("invalidDocument".into());
        }
        counts.add(row.status);
    }
    if counts != scan.summary.counts || scan.rows.len() as u64 != scan.summary.processed {
        return Err("invalidDocument".into());
    }
    // Loading historical data never touches its old source/reference drives.
    scan.roots = if scan.mode == "folders" {
        vec![&scan.source, &scan.backup]
    } else {
        vec![&scan.source]
    }
    .into_iter()
    .map(|p| Path::new(p).components().collect())
    .collect();
    Ok(scan)
}

pub fn all_match(snapshot: &Snapshot) -> bool {
    let c = &snapshot.summary.counts;
    !snapshot.summary.cancelled
        && snapshot.scope.exclusions.is_empty()
        && (snapshot.mode != "manifest" || snapshot.baseline_complete == Some(true))
        && c.matched > 0
        && c.different + c.missing + c.extra + c.error + c.skipped + c.unverified == 0
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonReport<'a> {
    schema_version: u8,
    application: &'static str,
    version: &'static str,
    scan_state: &'static str,
    all_files_match: bool,
    scan: &'a Snapshot,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn html(snapshot: &Snapshot) -> String {
    let c = &snapshot.summary.counts;
    let state = if snapshot.summary.cancelled {
        "Partial scan"
    } else if !snapshot.scope.exclusions.is_empty() || snapshot.baseline_complete == Some(false) {
        "Scoped verification"
    } else {
        "Completed scan"
    };
    let mut text = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'"><title>MirrorProof report</title><style>body{{font:14px/1.5 system-ui,sans-serif;background:#f3f3f0;color:#21272b;max-width:1200px;margin:32px auto;padding:0 24px}}header,section{{background:white;border:1px solid #d8dcde;border-radius:16px;padding:22px;margin-bottom:18px}}h1{{margin:0 0 6px}}p{{overflow-wrap:anywhere}}.muted{{color:#60636a}}table{{border-collapse:collapse;width:100%;font-size:12px;table-layout:fixed}}td,th{{padding:10px;text-align:left;border-bottom:1px solid #d8dcde;overflow-wrap:anywhere}}th{{background:#eceef0}}tr.matched td:nth-child(2){{color:#32624c}}tr:not(.matched) td:nth-child(2){{font-weight:600;color:#885c1a}}@media print{{body{{margin:0;padding:0;background:white}}header,section{{border:0;padding:8px;border-radius:0}}thead{{display:table-header-group}}tr{{break-inside:avoid}}}}</style></head><body><header><h1>MirrorProof</h1><strong>{state}</strong><p>{claim}</p><p>Source: {source}<br>Reference: {backup}</p><p class="muted">Scan timestamps (Unix seconds): {started}–{finished}<br>Mode: {mode}. Exclusions: {scope}</p></header><section><strong>Matched: {matched} · Different: {different} · Missing: {missing} · Extra: {extra} · Errors: {errors} · Skipped: {skipped} · Not verified: {unverified}</strong><p>{processed} / {total} results · {duration} ms · {read} bytes read</p><table><thead><tr><th>Relative path</th><th>Status</th><th>Reason / side</th><th>Source bytes</th><th>Reference bytes</th><th>OS error</th></tr></thead><tbody>"#,
        claim = if all_match(snapshot) {
            "All compared files matched SHA-256. This report describes the scan time."
        } else {
            "This report does not establish a complete whole-folder match. Review the scope and item results."
        },
        source = escape(&snapshot.source),
        backup = escape(&snapshot.backup),
        started = snapshot.started_at_unix_seconds,
        finished = snapshot.finished_at_unix_seconds,
        mode = escape(&snapshot.mode),
        scope = escape(&snapshot.scope.exclusions.join("; ")),
        matched = c.matched,
        different = c.different,
        missing = c.missing,
        extra = c.extra,
        errors = c.error,
        skipped = c.skipped,
        unverified = c.unverified,
        processed = snapshot.summary.processed,
        total = snapshot.summary.total,
        duration = snapshot.summary.duration_ms,
        read = snapshot.summary.bytes_read
    );
    for row in &snapshot.rows {
        text.push_str(&format!("<tr class=\"{}\"><td>{}</td><td>{}</td><td>{} / {}</td><td>{}</td><td>{}</td><td>{}</td></tr>",status_name(row.status),escape(&row.path),status_name(row.status),escape(row.reason),row.side,row.source_size.map(|v|v.to_string()).unwrap_or_else(||"—".into()),row.backup_size.map(|v|v.to_string()).unwrap_or_else(||"—".into()),row.os_code.map(|v|v.to_string()).unwrap_or_default()));
    }
    text.push_str("</tbody></table></section><p class=\"muted\">Generated by MirrorProof, developed by Crelyn Studio. Results are historical observations, not a filesystem snapshot or a new verification. Links/reparse points are not followed. Empty folders and filesystem metadata are not verified.</p></body></html>");
    text
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Matched => "matched",
        Status::Different => "different",
        Status::Missing => "missing",
        Status::Extra => "extra",
        Status::Error => "error",
        Status::Skipped => "skipped",
        Status::Unverified => "unverified",
    }
}

fn csv_cell(value: &str) -> String {
    let formula = value.trim_start().starts_with(['=', '+', '-', '@'])
        || value.starts_with(['\t', '\r', '\n']);
    format!(
        "\"{}{}\"",
        if formula { "'" } else { "" },
        value.replace('"', "\"\"")
    )
}

pub fn encode(snapshot: &Snapshot, format: &str) -> Result<Vec<u8>, String> {
    let state = if snapshot.summary.cancelled {
        "partial"
    } else {
        "completed"
    };
    let counts = &snapshot.summary.counts;
    if format == "html" {
        return Ok(html(snapshot).into_bytes());
    }
    if format == "json" {
        return serde_json::to_vec_pretty(&JsonReport {
            schema_version: 1,
            application: "MirrorProof",
            version: env!("CARGO_PKG_VERSION"),
            scan_state: state,
            all_files_match: all_match(snapshot),
            scan: snapshot,
        })
        .map_err(|_| "exportFailed".into());
    }
    if format != "csv" {
        return Err("exportInvalidFormat".into());
    }
    let mut text = String::from("\u{feff}source,backup,started_at_unix_seconds,finished_at_unix_seconds,scan_state,processed,total,matched,different,missing,extra,error,skipped,unverified,path,status,reason,side,source_size_bytes,backup_size_bytes,os_error,mode,exclusions,baseline_complete\r\n");
    let metadata = [
        snapshot.source.clone(),
        snapshot.backup.clone(),
        snapshot.started_at_unix_seconds.to_string(),
        snapshot.finished_at_unix_seconds.to_string(),
        state.into(),
        snapshot.summary.processed.to_string(),
        snapshot.summary.total.to_string(),
        counts.matched.to_string(),
        counts.different.to_string(),
        counts.missing.to_string(),
        counts.extra.to_string(),
        counts.error.to_string(),
        counts.skipped.to_string(),
        counts.unverified.to_string(),
    ]
    .iter()
    .map(|s| csv_cell(s))
    .collect::<Vec<_>>()
    .join(",");
    let mut add_row = |row: Option<&Row>| {
        let values = match row {
            Some(row) => vec![
                row.path.clone(),
                status_name(row.status).into(),
                row.reason.into(),
                row.side.into(),
                row.source_size.map(|s| s.to_string()).unwrap_or_default(),
                row.backup_size.map(|s| s.to_string()).unwrap_or_default(),
                row.os_code.map(|s| s.to_string()).unwrap_or_default(),
            ],
            None => vec![String::new(); 7],
        };
        text.push_str(&metadata);
        text.push(',');
        text.push_str(
            &values
                .iter()
                .map(|s| csv_cell(s))
                .collect::<Vec<_>>()
                .join(","),
        );
        text.push(',');
        text.push_str(&csv_cell(&snapshot.mode));
        text.push(',');
        text.push_str(&csv_cell(&snapshot.scope.exclusions.join("; ")));
        text.push(',');
        text.push_str(&csv_cell(
            &snapshot
                .baseline_complete
                .map(|v| v.to_string())
                .unwrap_or_default(),
        ));
        text.push_str("\r\n");
    };
    if snapshot.rows.is_empty() {
        add_row(None);
    } else {
        for row in &snapshot.rows {
            add_row(Some(row));
        }
    }
    Ok(text.into_bytes())
}

#[cfg(windows)]
pub(crate) fn inside(parent: &Path, root: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn CompareStringOrdinal(
            a: *const u16,
            a_len: i32,
            b: *const u16,
            b_len: i32,
            ignore_case: i32,
        ) -> i32;
    }
    let normalize = |p: &Path| {
        p.to_string_lossy()
            .strip_prefix("\\\\?\\")
            .unwrap_or(&p.to_string_lossy())
            .to_owned()
    };
    let parent: Vec<u16> = std::ffi::OsStr::new(&normalize(parent))
        .encode_wide()
        .collect();
    let mut root: Vec<u16> = std::ffi::OsStr::new(&normalize(root))
        .encode_wide()
        .collect();
    while root.last() == Some(&92) {
        root.pop();
    }
    parent.len() >= root.len()
        && unsafe {
            CompareStringOrdinal(
                parent.as_ptr(),
                root.len() as i32,
                root.as_ptr(),
                root.len() as i32,
                1,
            )
        } == 2
        && (parent.len() == root.len() || parent[root.len()] == 92)
}

#[cfg(not(windows))]
pub(crate) fn inside(parent: &Path, root: &Path) -> bool {
    parent.starts_with(root)
}

pub fn write_new(snapshot: &Snapshot, path: &Path, format: &str) -> Result<(), String> {
    let data = encode(snapshot, format)?;
    write_document(&snapshot.roots, path, format, &data)
}

pub fn write_document(
    roots: &[PathBuf],
    path: &Path,
    format: &str,
    data: &[u8],
) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("exportFolderUnsafe")?;
    // Reject Windows stream/device syntax so a report cannot modify an existing file's ADS.
    if name
        .chars()
        .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || name.ends_with([' ', '.'])
    {
        return Err("exportFolderUnsafe".into());
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || matches!(
        stem.strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT")),
        Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
    ) {
        return Err("exportFolderUnsafe".into());
    }
    if !matches!(format, "csv" | "json" | "html")
        || path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| !s.eq_ignore_ascii_case(format))
            .unwrap_or(true)
    {
        return Err("exportInvalidFormat".into());
    }
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("exportFolderUnsafe".into());
    }
    #[cfg(windows)]
    {
        use std::path::Prefix;
        if !matches!(path.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        {
            return Err("exportFolderUnsafe".into());
        }
    }
    let parent = path.parent().ok_or("exportFolderUnsafe")?;
    local_path(parent).map_err(|_| "exportFolderUnsafe")?;
    let _guards = pin_report_directory(parent)?;
    let parent = fs::canonicalize(parent).map_err(|_| "exportFolderUnsafe")?;
    if roots.is_empty() || roots.iter().any(|root| inside(&parent, root)) {
        return Err("exportInsideScan".into());
    }
    let destination = parent.join(path.file_name().ok_or("exportFolderUnsafe")?);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0).custom_flags(0x00200000);
    }
    let mut file = options.open(destination).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            "exportExists"
        } else {
            "exportFailed"
        }
    })?;
    // A failed write may leave an incomplete new report; existing files are never touched.
    file.write_all(data)
        .and_then(|_| file.sync_all())
        .map_err(|_| "exportFailed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{compare, Counts};
    use std::sync::atomic::AtomicBool;

    fn snapshot() -> (tempfile::TempDir, Snapshot) {
        let temp = tempfile::TempDir::new().unwrap();
        let a = temp.path().join("source");
        let b = temp.path().join("backup");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        fs::write(a.join("Bütünlük.txt"), "same").unwrap();
        fs::write(b.join("Bütünlük.txt"), "same").unwrap();
        let mut rows = Vec::new();
        let summary = compare(
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            &AtomicBool::new(false),
            |p| {
                rows.extend(p.rows);
                true
            },
        )
        .unwrap();
        let data = Snapshot {
            source: a.to_string_lossy().into_owned(),
            backup: b.to_string_lossy().into_owned(),
            started_at_unix_seconds: 1,
            finished_at_unix_seconds: 2,
            summary,
            rows,
            scope: Scope::default(),
            mode: "folders".into(),
            baseline_complete: None,
            roots: vec![fs::canonicalize(a).unwrap(), fs::canonicalize(b).unwrap()],
        };
        (temp, data)
    }

    #[test]
    fn exports_real_results_as_json_and_csv() {
        let (temp, data) = snapshot();
        for format in ["csv", "json"] {
            write_new(&data, &temp.path().join(format!("report.{format}")), format).unwrap();
        }
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(temp.path().join("report.json")).unwrap()).unwrap();
        assert_eq!(json["scanState"], "completed");
        assert_eq!(json["allFilesMatch"], true);
        assert_eq!(json["scan"]["rows"][0]["path"], "Bütünlük.txt");
        let csv = fs::read_to_string(temp.path().join("report.csv")).unwrap();
        assert!(csv.starts_with('\u{feff}'));
        assert!(csv.contains("\"Bütünlük.txt\",\"matched\",\"sha256Match\""));
        assert_eq!(
            fs::read(Path::new(&data.source).join("Bütünlük.txt")).unwrap(),
            b"same"
        );
    }

    #[test]
    fn html_is_standalone_escaped_and_scoped_reports_never_claim_a_full_match() {
        let (temp, mut data) = snapshot();
        data.rows[0].path = "<img src=x onerror=alert(1)>&\"".into();
        data.scope.exclusions = vec!["*.tmp".into()];
        let html = String::from_utf8(encode(&data, "html").unwrap()).unwrap();
        assert!(html.contains("&lt;img src=x"));
        assert!(!html.contains("<img src=x"));
        assert!(html.contains("Scoped verification"));
        assert!(html.contains("default-src 'none'"));
        let json: serde_json::Value =
            serde_json::from_slice(&encode(&data, "json").unwrap()).unwrap();
        assert_eq!(json["allFilesMatch"], false);
        write_new(&data, &temp.path().join("report.html"), "html").unwrap();
        let loaded = decode(&encode(&data, "json").unwrap()).unwrap();
        assert_eq!(loaded.scope.exclusions, vec!["*.tmp"]);
        let mut forged = json.clone();
        forged["scan"]["rows"][0]["path"] = "../escape".into();
        assert!(decode(&serde_json::to_vec(&forged).unwrap()).is_err());
        forged = json.clone();
        forged["scan"]["summary"]["counts"]["matched"] = 999.into();
        assert!(decode(&serde_json::to_vec(&forged).unwrap()).is_err());
        forged = json;
        forged["scan"]["rows"][0]["reason"] = "hashMismatch".into();
        assert!(decode(&serde_json::to_vec(&forged).unwrap()).is_err());
    }

    #[test]
    fn refuses_overwrites_and_scan_directories() {
        let (temp, data) = snapshot();
        let output = temp.path().join("existing.json");
        fs::write(&output, "keep").unwrap();
        assert_eq!(
            write_new(&data, &output, "json").unwrap_err(),
            "exportExists"
        );
        assert_eq!(fs::read_to_string(output).unwrap(), "keep");
        for root in [&data.source, &data.backup] {
            let path = Path::new(root).join("report.json");
            assert_eq!(
                write_new(&data, &path, "json").unwrap_err(),
                "exportInsideScan"
            );
            assert!(!path.exists());
        }
        assert_eq!(
            write_new(&data, &temp.path().join("report.exe"), "json").unwrap_err(),
            "exportInvalidFormat"
        );
    }

    #[test]
    fn partial_reports_never_claim_a_full_match() {
        let (_temp, mut data) = snapshot();
        data.summary.cancelled = true;
        let json: serde_json::Value =
            serde_json::from_slice(&encode(&data, "json").unwrap()).unwrap();
        assert_eq!(json["scanState"], "partial");
        assert_eq!(json["allFilesMatch"], false);
        assert!(String::from_utf8(encode(&data, "csv").unwrap())
            .unwrap()
            .contains("\"partial\""));
    }

    #[test]
    fn csv_escapes_formulas_quotes_delimiters_and_unicode() {
        for value in [
            "=1+1", "+cmd", "-1+2", "@SUM(1)", "  =1", "\ttext", "\ntext",
        ] {
            assert!(csv_cell(value).starts_with("\"'"));
        }
        assert_eq!(csv_cell("a,\"b\"\nTürkçe"), "\"a,\"\"b\"\"\nTürkçe\"");
    }

    #[test]
    fn empty_scan_exports_metadata_without_claiming_integrity() {
        let (_temp, mut data) = snapshot();
        data.rows.clear();
        data.summary.counts = Counts::default();
        data.summary.processed = 0;
        data.summary.total = 0;
        let json: serde_json::Value =
            serde_json::from_slice(&encode(&data, "json").unwrap()).unwrap();
        assert_eq!(json["allFilesMatch"], false);
        assert_eq!(
            String::from_utf8(encode(&data, "csv").unwrap())
                .unwrap()
                .lines()
                .count(),
            2
        );
    }

    #[test]
    fn folder_boundaries_are_respected() {
        let (_temp, data) = snapshot();
        assert!(inside(&data.roots[0].join("nested"), &data.roots[0]));
        assert!(inside(&data.roots[0], &data.roots[0]));
        let sibling = data.roots[0].with_file_name("source-other");
        assert!(!inside(&sibling, &data.roots[0]));
    }

    #[test]
    fn rejects_stream_syntax_and_device_names() {
        let (temp, data) = snapshot();
        let existing = temp.path().join("keep.json");
        fs::write(&existing, "untouched").unwrap();
        for name in [
            "keep.json:stream.json",
            "NUL.json",
            "CON.json",
            "COM1.csv",
            "report.json ",
        ] {
            let format = if name.ends_with("csv") { "csv" } else { "json" };
            assert_eq!(
                write_new(&data, &temp.path().join(name), format).unwrap_err(),
                "exportFolderUnsafe"
            );
        }
        assert_eq!(fs::read_to_string(existing).unwrap(), "untouched");
    }

    #[cfg(windows)]
    #[test]
    fn refuses_junction_export_destinations_without_showing_a_console() {
        use std::os::windows::process::CommandExt;
        let (temp, data) = snapshot();
        let alias = temp.path().join("alias");
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&alias)
            .arg(&data.source)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(output.status.success());
        let result = write_new(&data, &alias.join("report.json"), "json");
        fs::remove_dir(&alias).unwrap();
        assert_eq!(result.unwrap_err(), "exportFolderUnsafe");
        assert!(!Path::new(&data.source).join("report.json").exists());
    }
}
