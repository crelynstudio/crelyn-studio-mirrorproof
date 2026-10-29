import { useRef, useState } from 'react';
import { Channel, invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
import { FolderOpen, SlidersHorizontal, X } from 'lucide-react';
import { copy as t } from './copy';
import { validExclusions } from './preferences';
import type { Preferences } from './preferences';
import { readSavedPairs } from './pairStorage';
import type { SavedPair } from './pairStorage';
import type { Progress, Report, Snapshot } from './scanTypes';

export type QueueResult = { pair: SavedPair; summary?: Report; error?: string };
type Schedule = { id: string; name: string; source: string; backup: string; scope: { exclusions: string[] }; frequency: string; time: string; weekday: number; executable: string };
type ManifestOutcome = { files: number; complete: boolean; issues: number; snapshot: Snapshot | null; historyError: string | null };
const weekdays = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];

export default function ToolsDialog({ disabled, source, preferences, queueResults, onPreferences, onQueue, onSnapshot, onHistory, onBusy }: {
  disabled: boolean; source: string; preferences: Preferences; queueResults: QueueResult[];
  onPreferences: (value: Preferences) => void; onQueue: (pairs: SavedPair[]) => void;
  onSnapshot: (snapshot: Snapshot) => void; onHistory: (id: string) => Promise<void>; onBusy: (busy: boolean) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [tab, setTab] = useState<'options' | 'queue' | 'manifests' | 'schedules'>('options');
  const [rules, setRules] = useState(''), [notify, setNotify] = useState(false), [error, setError] = useState('');
  const [pairs, setPairs] = useState<SavedPair[]>([]), [selected, setSelected] = useState<string[]>([]);
  const [folder, setFolder] = useState(''), [baseline, setBaseline] = useState('');
  const [working, setWorking] = useState(false), [progress, setProgress] = useState<Progress | null>(null), [manifest, setManifest] = useState<ManifestOutcome | null>(null), [manifestPath, setManifestPath] = useState('');
  const [jobs, setJobs] = useState<Schedule[]>([]), [pairId, setPairId] = useState(''), [frequency, setFrequency] = useState('daily'), [time, setTime] = useState('19:00'), [weekday, setWeekday] = useState(1);

  async function show() {
    setError(''); setRules(preferences.exclusions.join('\n')); setNotify(preferences.notify); setFolder(source); setProgress(null); setManifest(null); setSelected([]);
    try { const saved = readSavedPairs(localStorage); setPairs(saved); setPairId(saved[0]?.id ?? ''); setJobs(await invoke<Schedule[]>('list_schedules')); } catch { setError(t.storageFailed); }
    dialog.current?.showModal();
  }
  function busy(value: boolean) { setWorking(value); onBusy(value); }
  async function apply() {
    const exclusions = rules.split(/\r?\n/).map(s => s.trim()).filter(Boolean);
    if (!validExclusions(exclusions)) { setError(t.invalidExclusions); return; }
    busy(true); setError('');
    try {
      if (notify && !await isPermissionGranted() && await requestPermission() !== 'granted') { setError(t.notificationFailed); return; }
      onPreferences({ exclusions, notify }); dialog.current?.close();
    } catch { setError(t.settingsWriteFailed); } finally { busy(false); }
  }
  async function choose(which: 'folder' | 'baseline') {
    busy(true); setError('');
    try {
      const path = await open(which === 'folder' ? { directory: true, multiple: false } : { multiple: false, filters: [{ name: 'MirrorProof SHA-256 manifest', extensions: ['json'] }] });
      if (typeof path === 'string') { (which === 'folder' ? setFolder : setBaseline)(path); setManifest(null); }
    } catch { setError(t.pickerFailed); } finally { busy(false); }
  }
  async function runManifest(action: 'create' | 'verify') {
    busy(true); setError(''); setManifest(null); setProgress(null); setManifestPath('');
    try {
      const path = action === 'create' ? await save({ title: t.saveManifestOutside, defaultPath: 'MirrorProof-manifest.json', filters: [{ name: 'SHA-256 manifest', extensions: ['json'] }] }) : baseline;
      if (!path) return;
      const channel = new Channel<Progress>(); channel.onmessage = setProgress;
      const result = await invoke<ManifestOutcome>('manifest_job', { action, folder, path, scope: { exclusions: preferences.exclusions }, onProgress: channel });
      setManifest(result); setManifestPath(path);
      // The native export snapshot changes on verification, so keep the workspace in sync even if this dialog is dismissed.
      if (result.snapshot) onSnapshot(result.snapshot);
      if (result.historyError) setError(t.historySaveFailed);
      if (preferences.notify) {
        try { await sendNotification({ title: 'MirrorProof', body: result.snapshot ? t.manifestVerified : t.manifestCreated }); } catch { setError(t.notificationFailed); }
      }
    } catch (cause) { const key = String(cause); setError(Object.hasOwn(t, key) ? t[key as keyof typeof t] : t.invalidManifest); } finally { busy(false); }
  }
  async function createSchedule() {
    const pair = pairs.find(p => p.id === pairId); if (!pair) return;
    busy(true); setError('');
    try {
      const plan = await invoke<Schedule>('create_schedule', { plan: { id: '', name: pair.name, source: pair.source, backup: pair.backup, scope: { exclusions: preferences.exclusions }, frequency, time, weekday, executable: '' } });
      setJobs([...jobs, plan]);
    } catch { setError(t.scheduleFailed); } finally { busy(false); }
  }
  async function removeSchedule(id: string) {
    busy(true); setError('');
    try { await invoke('remove_schedule', { id }); setJobs(jobs.filter(j => j.id !== id)); } catch { setError(t.scheduleFailed); } finally { busy(false); }
  }

  return <>
    <button className="button secondary heading-button" disabled={disabled} onClick={() => void show()}><SlidersHorizontal size={14} />{t.tools}</button>
    <dialog ref={dialog} className="tools-dialog" aria-labelledby="tools-title" onCancel={event => { if (working) event.preventDefault(); }}>
      <div className="dialog-heading"><h2 id="tools-title">{t.tools}</h2><button className="icon-button" aria-label={t.close} disabled={working} onClick={() => dialog.current?.close()} autoFocus><X size={20} /></button></div>
      <nav className="tool-tabs" aria-label={t.tools}>{(['options', 'queue', 'manifests', 'schedules'] as const).map(name => <button key={name} className={tab === name ? 'active' : ''} disabled={working} aria-pressed={tab === name} onClick={() => { setTab(name); setError(''); }}>{t[name]}</button>)}</nav>
      {error && <p className="dialog-error" role="alert">{error}</p>}
      {tab === 'options' && <section className="tool-panel"><label htmlFor="exclusion-rules">{t.exclusions}</label><textarea id="exclusion-rules" rows={4} value={rules} onChange={e => setRules(e.target.value)} placeholder={'*.tmp\ncache/\n**/node_modules/**'} disabled={working} /><p className="dialog-help">{t.exclusionsHint}</p><label className="checkbox-label"><input type="checkbox" checked={notify} onChange={e => setNotify(e.target.checked)} disabled={working} />{t.completionNotifications}</label><p className="dialog-help">{t.notificationHint}</p><button className="button export-button" disabled={working} onClick={() => void apply()}>{t.applyOptions}</button></section>}
      {tab === 'queue' && <section className="tool-panel"><p className="dialog-help">{t.queueHint}</p><ul className="queue-pairs">{pairs.map(pair => <li key={pair.id}><label className="checkbox-label"><input type="checkbox" checked={selected.includes(pair.id)} onChange={e => setSelected(e.target.checked ? [...selected, pair.id] : selected.filter(id => id !== pair.id))} />{pair.name}</label></li>)}</ul>{!pairs.length && <p className="dialog-help">{t.noSavedPairs}</p>}<button className="button export-button" disabled={!selected.length || working} onClick={() => { dialog.current?.close(); onQueue(pairs.filter(p => selected.includes(p.id))); }}>{t.runQueue} ({selected.length})</button>{queueResults.length > 0 && <div className="queue-results"><h3>{t.queueResults}</h3>{queueResults.map((r, i) => <div key={i}><strong>{r.pair.name}</strong><small>{r.error ? (Object.hasOwn(t, r.error) ? t[r.error as keyof typeof t] : t.scanFailed) : r.summary?.cancelled ? t.cancelled : `${t.done} · ${r.summary?.processed} ${t.resultsUnit}`}{r.summary?.historyError ? ` · ${t.historySaveFailed}` : ''}</small>{r.summary?.historyId && <button className="button secondary" onClick={() => { void onHistory(r.summary!.historyId!).then(() => dialog.current?.close()).catch(() => setError(t.invalidDocument)); }}>{t.openResult}</button>}</div>)}</div>}</section>}
      {tab === 'manifests' && <section className="tool-panel"><p className="dialog-help">{t.manifestHint}</p><div className="tool-path"><span title={folder}>{folder || t.folderNotSelected}</span><button className="button secondary" disabled={working} onClick={() => void choose('folder')}><FolderOpen size={14} />{t.choose}</button></div><div className="tool-path"><span title={baseline}>{baseline || t.noManifest}</span><button className="button secondary" disabled={working} onClick={() => void choose('baseline')}>{t.openManifest}</button></div><div className="history-actions"><button className="button secondary" disabled={!folder || working} onClick={() => void runManifest('create')}>{t.createManifest}</button><button className="button export-button" disabled={!folder || !baseline || working} onClick={() => void runManifest('verify')}>{t.verifyManifest}</button></div>{working && <div className="manifest-progress"><p role="status">{progress ? `${t[progress.phase === 'hashing' ? 'hashing' : progress.phase === 'checking' ? 'checking' : 'indexing']} ${progress.processed} / ${progress.total}` : t.loading}</p><p title={progress?.currentPath}>{progress?.currentPath}</p><button className="button secondary" onClick={() => { void invoke('cancel_scan').catch(() => setError(t.cancelFailed)); }}>{t.cancel}</button></div>}{manifest && <div className="manifest-outcome" role="status"><strong>{manifest.snapshot ? t.manifestVerified : t.manifestCreated}</strong><p>{manifest.files} {t.resultsUnit} · {manifest.complete ? t.fullScope : t.scoped}</p><p>{manifestPath}</p>{manifest.snapshot && <button className="button secondary" onClick={() => { onSnapshot(manifest.snapshot!); dialog.current?.close(); }}>{t.openResult}</button>}</div>}</section>}
      {tab === 'schedules' && <section className="tool-panel">
        <p className="dialog-help">{t.scheduleHint}</p>
        <div className="schedule-form">
          <div><label htmlFor="schedule-pair">{t.savedPairs}</label><select id="schedule-pair" value={pairId} disabled={working} onChange={e => setPairId(e.target.value)}>{pairs.map(pair => <option key={pair.id} value={pair.id}>{pair.name}</option>)}</select></div>
          <div><label htmlFor="schedule-frequency">{t.frequency}</label><select id="schedule-frequency" value={frequency} disabled={working} onChange={e => setFrequency(e.target.value)}><option value="daily">{t.daily}</option><option value="weekly">{t.weekly}</option></select></div>
          <div><label htmlFor="schedule-time">{t.localTime}</label><input id="schedule-time" type="time" value={time} disabled={working} onChange={e => setTime(e.target.value)} /></div>
          {frequency === 'weekly' && <div><label htmlFor="schedule-day">{t.weekday}</label><select id="schedule-day" value={weekday} disabled={working} onChange={e => setWeekday(Number(e.target.value))}>{weekdays.map((day, i) => <option value={i} key={day}>{day}</option>)}</select></div>}
        </div>
        <button className="button export-button" disabled={!pairId || !time || working} onClick={() => void createSchedule()}>{t.enableSchedule}</button>
        <ul className="schedule-list">{jobs.map(job => <li key={job.id}><div><strong>{job.name}</strong><p>{job.frequency === 'daily' ? t.daily : `${t.weekly} · ${weekdays[job.weekday]}`} · {job.time}</p><small title={job.executable}>{job.executable}</small></div><button className="button secondary" disabled={working} onClick={() => void removeSchedule(job.id)}>{t.removeSchedule}</button></li>)}</ul>
        {!jobs.length && <p className="dialog-help">{t.noSchedules}</p>}
      </section>}
    </dialog>
  </>;
}
