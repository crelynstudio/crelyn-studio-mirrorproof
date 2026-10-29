mod engine;
mod history;
mod reports;
mod schedules;
mod scope;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{ipc::Channel, State};
use tauri_plugin_opener::OpenerExt;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanOutcome {
    #[serde(flatten)]
    summary: engine::Report,
    history_error: Option<String>,
    history_id: Option<String>,
}

#[tauri::command]
async fn list_schedules() -> Result<Vec<schedules::Plan>, String> {
    tauri::async_runtime::spawn_blocking(schedules::list)
        .await
        .map_err(|_| "scheduleFailed".to_string())?
}
#[tauri::command]
async fn create_schedule(plan: schedules::Plan) -> Result<schedules::Plan, String> {
    tauri::async_runtime::spawn_blocking(move || schedules::create(plan))
        .await
        .map_err(|_| "scheduleFailed".to_string())?
}
#[tauri::command]
async fn remove_schedule(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || schedules::remove(&id))
        .await
        .map_err(|_| "scheduleFailed".to_string())?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestOutcome {
    files: usize,
    complete: bool,
    issues: usize,
    snapshot: Option<reports::Snapshot>,
    history_error: Option<String>,
}

#[tauri::command]
async fn manifest_job(
    action: String,
    folder: String,
    path: String,
    scope: Option<scope::Scope>,
    on_progress: Channel<engine::Progress>,
    control: State<'_, Arc<Control>>,
) -> Result<ManifestOutcome, String> {
    let control = Arc::clone(control.inner());
    if control
        .running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("busy".into());
    }
    let guard = RunningGuard(control.clone());
    control.cancel.store(false, Ordering::Release);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        match action.as_str() {
            "create" => {
                let manifest = engine::manifest::build(
                    &folder,
                    &scope.unwrap_or_default(),
                    &control.cancel,
                    |p| on_progress.send(p).is_ok(),
                )?;
                if manifest.cancelled {
                    return Err("manifestCancelled".into());
                }
                let root = std::fs::canonicalize(&folder).map_err(|_| "invalidFolder")?;
                reports::write_document(
                    &[root],
                    std::path::Path::new(&path),
                    "json",
                    &serde_json::to_vec_pretty(&manifest).map_err(|_| "exportFailed")?,
                )?;
                Ok(ManifestOutcome {
                    files: manifest.files.len(),
                    complete: manifest.complete,
                    issues: manifest.issues.len(),
                    snapshot: None,
                    history_error: None,
                })
            }
            "verify" => {
                let snapshot = engine::manifest::verify(
                    &folder,
                    std::path::Path::new(&path),
                    &control.cancel,
                    |p| on_progress.send(p).is_ok(),
                )?;
                let history_error = history::save(&snapshot).err();
                let outcome = ManifestOutcome {
                    files: snapshot.rows.len(),
                    complete: reports::all_match(&snapshot),
                    issues: (snapshot.summary.counts.error
                        + snapshot.summary.counts.skipped
                        + snapshot.summary.counts.unverified) as usize,
                    snapshot: Some(snapshot.clone()),
                    history_error,
                };
                *control.report.lock().map_err(|_| "workerFailed")? = Some(Arc::new(snapshot));
                Ok(outcome)
            }
            _ => Err("invalidManifest".into()),
        }
    })
    .await
    .map_err(|_| "workerFailed".to_string())?
}

pub fn run_scheduled(id: &str) -> Result<bool, String> {
    schedules::run(id)
}

pub fn remove_schedules() -> Result<(), String> {
    for plan in schedules::list()? {
        schedules::remove(&plan.id)?;
    }
    Ok(())
}

#[derive(Default)]
struct Control {
    running: AtomicBool,
    cancel: AtomicBool,
    report: Mutex<Option<Arc<reports::Snapshot>>>,
}

struct RunningGuard(Arc<Control>);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::Release);
    }
}

#[tauri::command]
async fn scan(
    source: String,
    backup: String,
    scope: Option<scope::Scope>,
    on_progress: Channel<engine::Progress>,
    control: State<'_, Arc<Control>>,
) -> Result<ScanOutcome, String> {
    let control = Arc::clone(control.inner());
    if control
        .running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("busy".into());
    }
    let guard = RunningGuard(control.clone());
    control.cancel.store(false, Ordering::Release);
    *control.report.lock().map_err(|_| "workerFailed")? = None;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let started = reports::now();
        let roots = [&source, &backup]
            .iter()
            .filter_map(|p| std::fs::canonicalize(p).ok())
            .collect();
        let mut rows = Vec::new();
        let scope = scope.unwrap_or_default();
        let result = engine::compare_scoped(
            &source,
            &backup,
            &control.cancel,
            |progress| {
                rows.extend(progress.rows.iter().cloned());
                on_progress.send(progress).is_ok()
            },
            &scope,
        );
        match result {
            Ok(summary) => {
                let snapshot = reports::Snapshot {
                    source,
                    backup,
                    roots,
                    started_at_unix_seconds: started,
                    finished_at_unix_seconds: reports::now(),
                    summary: summary.clone(),
                    rows,
                    scope,
                    mode: "folders".into(),
                    baseline_complete: None,
                };
                let saved = history::save(&snapshot);
                let history_error = saved.as_ref().err().cloned();
                let history_id = saved.ok();
                *control.report.lock().map_err(|_| "workerFailed")? = Some(Arc::new(snapshot));
                Ok(ScanOutcome {
                    summary,
                    history_error,
                    history_id,
                })
            }
            Err(error) => {
                history::failed(&source, &backup, &scope, &error);
                Err(error)
            }
        }
    })
    .await
    .map_err(|_| "workerFailed".to_string())?
}

#[tauri::command]
async fn list_history() -> Result<Vec<history::Entry>, String> {
    tauri::async_runtime::spawn_blocking(|| history::list_at(&history::data_dir()?))
        .await
        .map_err(|_| "storageFailed".to_string())?
}

#[tauri::command]
async fn current_report(control: State<'_, Arc<Control>>) -> Result<reports::Snapshot, String> {
    control
        .report
        .lock()
        .map_err(|_| "workerFailed")?
        .as_ref()
        .map(|s| s.as_ref().clone())
        .ok_or_else(|| "exportNoReport".into())
}

#[tauri::command]
async fn load_report(
    id: Option<String>,
    path: Option<String>,
    activate: Option<bool>,
    control: State<'_, Arc<Control>>,
) -> Result<reports::Snapshot, String> {
    let control = Arc::clone(control.inner());
    if control
        .running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("busy".into());
    }
    let guard = RunningGuard(control.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let snapshot = match (id, path) {
            (Some(id), None) => history::load_at(&history::data_dir()?, &id)?,
            (None, Some(path)) => {
                reports::decode(&reports::read_document(std::path::Path::new(&path))?)?
            }
            _ => return Err("invalidDocument".into()),
        };
        if activate.unwrap_or(true) {
            *control.report.lock().map_err(|_| "workerFailed")? = Some(Arc::new(snapshot.clone()));
        }
        Ok(snapshot)
    })
    .await
    .map_err(|_| "workerFailed".to_string())?
}

#[tauri::command]
async fn reveal_file(
    path: String,
    side: String,
    app: tauri::AppHandle,
    control: State<'_, Arc<Control>>,
) -> Result<String, String> {
    let snapshot = control
        .report
        .lock()
        .map_err(|_| "workerFailed")?
        .clone()
        .ok_or("exportNoReport")?;
    if !matches!(side.as_str(), "source" | "backup") || !reports::valid_relative(&path) {
        return Err("revealFailed".into());
    }
    let row = snapshot
        .rows
        .iter()
        .find(|r| r.path == path)
        .ok_or("revealFailed")?;
    if row.status == engine::Status::Skipped
        || (snapshot.mode == "folders"
            && ((row.status == engine::Status::Missing && side == "backup")
                || (row.status == engine::Status::Extra && side == "source")))
        || (snapshot.mode == "manifest"
            && row.status == engine::Status::Missing
            && side == "source")
    {
        return Err("revealFailed".into());
    }
    let root = if side == "source" {
        &snapshot.source
    } else {
        &snapshot.backup
    };
    let item = if side == "backup" && snapshot.mode == "manifest" {
        std::path::PathBuf::from(root)
    } else {
        std::path::Path::new(root).join(path)
    };
    reports::local_path(&item)?;
    let _guards = engine::pin_report_directory(item.parent().ok_or("revealFailed")?)
        .map_err(|_| "revealFailed")?;
    let meta = std::fs::symlink_metadata(&item).map_err(|_| "revealFailed")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err("revealFailed".into());
        }
    }
    if !meta.is_file() && !meta.is_dir() {
        return Err("revealFailed".into());
    }
    #[cfg(debug_assertions)]
    if std::env::var_os("MIRRORPROOF_HEADLESS_TEST").is_some() {
        return Ok(item.to_string_lossy().into_owned());
    }
    app.opener()
        .reveal_item_in_dir(&item)
        .map_err(|_| "revealFailed")?;
    Ok(item.to_string_lossy().into_owned())
}

#[tauri::command]
fn cancel_scan(control: State<'_, Arc<Control>>) {
    control.cancel.store(true, Ordering::Release);
}

#[tauri::command]
async fn export_report(
    path: String,
    format: String,
    control: State<'_, Arc<Control>>,
) -> Result<(), String> {
    if control.running.load(Ordering::Acquire) {
        return Err("busy".into());
    }
    let snapshot = control
        .report
        .lock()
        .map_err(|_| "exportFailed")?
        .clone()
        .ok_or("exportNoReport")?;
    tauri::async_runtime::spawn_blocking(move || {
        reports::write_new(&snapshot, std::path::Path::new(&path), &format)
    })
    .await
    .map_err(|_| "exportFailed".to_string())?
}

pub fn run() {
    tauri::Builder::default()
        .manage(Arc::new(Control::default()))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            scan,
            cancel_scan,
            export_report,
            list_history,
            load_report,
            current_report,
            reveal_file,
            manifest_job,
            list_schedules,
            create_schedule,
            remove_schedule
        ])
        .run(tauri::generate_context!())
        .expect("MirrorProof could not start");
}
