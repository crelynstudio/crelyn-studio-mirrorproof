use crate::{
    engine, history,
    reports::{self, Snapshot},
    scope::Scope,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub name: String,
    pub source: String,
    pub backup: String,
    pub scope: Scope,
    pub frequency: String,
    pub time: String,
    pub weekday: u8,
    pub executable: String,
}

impl Plan {
    fn validate(&self) -> Result<(), String> {
        if !history::valid_id(&self.id)
            || self.name.trim().is_empty()
            || self.name.len() > 320
            || !matches!(self.frequency.as_str(), "daily" | "weekly")
            || self.weekday > 6
        {
            return Err("invalidSchedule".into());
        }
        let parts: Vec<_> = self.time.split(':').collect();
        if parts.len() != 2
            || parts
                .iter()
                .any(|s| s.len() != 2 || !s.bytes().all(|b| b.is_ascii_digit()))
            || parts[0].parse::<u8>().unwrap_or(99) > 23
            || parts[1].parse::<u8>().unwrap_or(99) > 59
        {
            return Err("invalidSchedule".into());
        }
        self.scope.validate()?;
        reports::path_syntax(std::path::Path::new(&self.source))?;
        reports::path_syntax(std::path::Path::new(&self.backup))?;
        reports::path_syntax(std::path::Path::new(&self.executable))?;
        Ok(())
    }
}

const SCRIPT: &str = r#"$ErrorActionPreference='Stop'
$record=[Console]::In.ReadToEnd()|ConvertFrom-Json
$service=New-Object -ComObject 'Schedule.Service';$service.Connect();$folder=$service.GetFolder('\')
$taskName='MirrorProof-'+$record.plan.id
if($record.action -eq 'remove') {
  try {$folder.DeleteTask($taskName,0)} catch {$cause=$_.Exception;while($cause.InnerException){$cause=$cause.InnerException};if($cause.HResult -ne -2147024894){throw}}
  exit 0
}
$definition=$service.NewTask(0)
$definition.RegistrationInfo.Description='MirrorProof read-only backup verification'
$definition.Principal.UserId=[System.Security.Principal.WindowsIdentity]::GetCurrent().Name
$definition.Principal.LogonType=3;$definition.Principal.RunLevel=0
$definition.Settings.Enabled=$true;$definition.Settings.StartWhenAvailable=$true
if($record.action -eq 'create-test'){$definition.Settings.Enabled=$false}
$definition.Settings.DisallowStartIfOnBatteries=$false;$definition.Settings.StopIfGoingOnBatteries=$false
$definition.Settings.MultipleInstances=2;$definition.Settings.ExecutionTimeLimit='PT0S'
$when=[datetime]::Today.Add([timespan]::Parse($record.plan.time))
if($record.plan.frequency -eq 'weekly') {
  while([int]$when.DayOfWeek -ne [int]$record.plan.weekday){$when=$when.AddDays(1)}
  if($when -le [datetime]::Now){$when=$when.AddDays(7)}
  $trigger=$definition.Triggers.Create(3);$trigger.WeeksInterval=1;$trigger.DaysOfWeek=[int][math]::Pow(2,[int]$record.plan.weekday)
} else {
  if($when -le [datetime]::Now){$when=$when.AddDays(1)}
  $trigger=$definition.Triggers.Create(2);$trigger.DaysInterval=1
}
$trigger.StartBoundary=$when.ToString('yyyy-MM-ddTHH:mm:ss')
$action=$definition.Actions.Create(0);$action.Path=$record.plan.executable
$action.Arguments='--scheduled '+$record.plan.id
$action.WorkingDirectory=[System.IO.Path]::GetDirectoryName($record.plan.executable)
# Validate the definition before registering; no password, elevation or executable shell text.
$null=$definition.XmlText
if($record.action -eq 'validate'){exit 0}
$null=$folder.RegisterTaskDefinition($taskName,$definition,2,$null,$null,3,$null)
"#;

fn scheduler(action: &str, plan: &Plan) -> Result<(), String> {
    plan.validate()?;
    #[cfg(not(windows))]
    {
        let _ = (action, plan);
        return Err("scheduleFailed".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let effective_action = if cfg!(debug_assertions)
            && action == "create"
            && std::env::var_os("MIRRORPROOF_HEADLESS_TEST").is_some()
        {
            "create-test"
        } else {
            action
        };
        let text =
            serde_json::to_string(&serde_json::json!({"action":effective_action,"plan":plan}))
                .map_err(|_| "scheduleFailed")?;
        // ASCII JSON avoids the system codepage corrupting Unicode executable paths on stdin.
        let mut payload = String::new();
        for c in text.chars() {
            if c.is_ascii() {
                payload.push(c);
            } else {
                for unit in c.to_string().encode_utf16() {
                    payload.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
        let shell = std::env::var_os("SystemRoot")
            .map(|p| PathBuf::from(p).join("System32/WindowsPowerShell/v1.0/powershell.exe"))
            .ok_or("scheduleFailed")?;
        let mut child = Command::new(shell)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                SCRIPT,
            ])
            .creation_flags(0x08000000)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| "scheduleFailed")?;
        child
            .stdin
            .take()
            .ok_or("scheduleFailed")?
            .write_all(payload.as_bytes())
            .map_err(|_| "scheduleFailed")?;
        if !child
            .wait_with_output()
            .map_err(|_| "scheduleFailed")?
            .status
            .success()
        {
            return Err("scheduleFailed".into());
        }
        Ok(())
    }
}

fn folder() -> Result<PathBuf, String> {
    Ok(history::data_dir()?.join("schedules"))
}
fn load(id: &str) -> Result<Plan, String> {
    if !history::valid_id(id) {
        return Err("invalidSchedule".into());
    }
    if disabled(&folder()?, id) {
        return Err("invalidSchedule".into());
    }
    let plan: Plan = serde_json::from_slice(&reports::read_document(
        &folder()?.join(format!("{id}.json")),
    )?)
    .map_err(|_| "invalidSchedule")?;
    plan.validate()?;
    if plan.id != id {
        return Err("invalidSchedule".into());
    }
    Ok(plan)
}
fn disabled(base: &std::path::Path, id: &str) -> bool {
    fs::symlink_metadata(base.join(format!("{id}.disabled.json"))).is_ok()
}
pub fn list() -> Result<Vec<Plan>, String> {
    let base = folder()?;
    if !base.exists() {
        return Ok(Vec::new());
    }
    let _pins = engine::pin_report_directory(&base)?;
    let mut plans = Vec::new();
    for item in fs::read_dir(&base).map_err(|_| "storageFailed")? {
        let item = item.map_err(|_| "storageFailed")?;
        let name = item.file_name();
        if let Some(id) = name
            .to_str()
            .and_then(|s| s.strip_suffix(".json"))
            .filter(|s| history::valid_id(s))
        {
            if disabled(&base, id) {
                continue;
            }
            plans.push(load(id)?);
        }
    }
    Ok(plans)
}
pub fn create(mut plan: Plan) -> Result<Plan, String> {
    plan.id = history::id();
    plan.executable = std::env::current_exe()
        .map_err(|_| "scheduleFailed")?
        .to_string_lossy()
        .into_owned();
    plan.validate()?;
    let (a, b, _guards) = engine::roots(&plan.source, &plan.backup)?;
    let base = folder()?;
    if [a.clone(), b.clone()].iter().any(|root| {
        reports::inside(&base, root)
            || reports::inside(&base.parent().unwrap_or(&base).join("history"), root)
    }) {
        return Err("historyInsideScan".into());
    }
    let _pins = history::safe_dir(&base)?;
    // Persist an inactive plan first; registration failure cannot leave an active local schedule.
    let pending = base.join(format!("{}.pending.json", plan.id));
    let roots = [a, b];
    let bytes = serde_json::to_vec(&plan).map_err(|_| "storageFailed")?;
    reports::write_document(&roots, &pending, "json", &bytes)?;
    scheduler("create", &plan)?;
    // Guarded directories are never relaxed for renames: append a new active record instead.
    if reports::write_document(
        &roots,
        &base.join(format!("{}.json", plan.id)),
        "json",
        &bytes,
    )
    .is_err()
    {
        let _ = scheduler("remove", &plan);
        return Err("storageFailed".into());
    }
    Ok(plan)
}
pub fn remove(id: &str) -> Result<(), String> {
    if !history::valid_id(id) {
        return Err("invalidSchedule".into());
    }
    if disabled(&folder()?, id) {
        return Ok(());
    }
    let plan = load(id)?;
    scheduler("remove", &plan)?;
    let base = folder()?;
    let _pins = engine::pin_report_directory(&base)?;
    // A tombstone prevents future CLI runs without deleting or overwriting the stored plan.
    match reports::write_document(
        &[PathBuf::from(&plan.source), PathBuf::from(&plan.backup)],
        &base.join(format!("{id}.disabled.json")),
        "json",
        b"{}",
    ) {
        Ok(()) => Ok(()),
        Err(error) if error == "exportExists" => Ok(()),
        Err(_) => Err("storageFailed".into()),
    }
}

pub fn run(id: &str) -> Result<bool, String> {
    let plan = load(id)?;
    let started = reports::now();
    let mut rows = Vec::new();
    let result = engine::compare_scoped(
        &plan.source,
        &plan.backup,
        &AtomicBool::new(false),
        |p| {
            rows.extend(p.rows);
            true
        },
        &plan.scope,
    );
    match result {
        Ok(summary) => {
            let roots = [&plan.source, &plan.backup]
                .iter()
                .map(|p| fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p)))
                .collect();
            let snapshot = Snapshot {
                source: plan.source,
                backup: plan.backup,
                started_at_unix_seconds: started,
                finished_at_unix_seconds: reports::now(),
                summary,
                rows,
                scope: plan.scope,
                mode: "folders".into(),
                baseline_complete: None,
                roots,
            };
            history::save(&snapshot)?;
            Ok(reports::all_match(&snapshot))
        }
        Err(error) => {
            history::failed(&plan.source, &plan.backup, &plan.scope, &error);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> Plan {
        Plan {
            id: "123456".into(),
            name: "Photos & backup".into(),
            source: "C:\\Source".into(),
            backup: "D:\\Backup".into(),
            scope: Scope::default(),
            frequency: "daily".into(),
            time: "19:30".into(),
            weekday: 1,
            executable: "C:\\Program Files\\MirrorProof.exe".into(),
        }
    }
    #[test]
    fn schedule_validation_rejects_injection_and_invalid_times() {
        let mut p = plan();
        p.validate().unwrap();
        p.id = "1;calc".into();
        assert!(p.validate().is_err());
        p = plan();
        p.time = "25:30".into();
        assert!(p.validate().is_err());
        p = plan();
        p.weekday = 7;
        assert!(p.validate().is_err());
        assert!(!SCRIPT.contains("Invoke-Expression"));
    }
    #[cfg(windows)]
    #[test]
    fn validates_real_windows_daily_and_weekly_definitions_without_registering_tasks() {
        let mut p = plan();
        scheduler("validate", &p).unwrap();
        p.frequency = "weekly".into();
        scheduler("validate", &p).unwrap();
    }
}
