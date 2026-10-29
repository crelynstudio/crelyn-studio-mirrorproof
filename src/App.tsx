import { Fragment, useDeferredValue, useEffect, useMemo, useRef, useState } from 'react';
import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { open, save } from '@tauri-apps/plugin-dialog';
import { sendNotification } from '@tauri-apps/plugin-notification';
import { Check, ChevronLeft, ChevronRight, CircleAlert, CircleCheck, Copy, FileCheck, FileDiff, FileMinus, FileOutput, FilePlus, FileText as File, FolderCheck, FolderInput, FolderOpen, Info, LoaderCircle, Minus, Play, ScanLine, Search, Square, X } from 'lucide-react';
import { copy } from './copy';
import type { Text } from './copy';
import type { Status, Counts, Row, Progress, Report, Snapshot, ViewContext } from './scanTypes';
import type { ReadSample } from './scanStats';
import { estimateRead } from './scanStats';
import { bytes, duration } from './format';
import FileDetails from './FileDetails';
import SavedPairs from './SavedPairs';
import DeveloperCard from './DeveloperCard';
import HistoryDialog from './HistoryDialog';
import ToolsDialog from './ToolsDialog';
import type { QueueResult } from './ToolsDialog';
import type { SavedPair } from './pairStorage';
import { preferenceKey, readPreferences } from './preferences';
import appMark from './assets/app-icon.svg';
import wordmark from './assets/mirrorproof-wordmark.svg';

const statuses: Status[] = ['matched', 'different', 'missing', 'extra', 'error', 'skipped', 'unverified'];
const emptyCounts: Counts = { matched: 0, different: 0, missing: 0, extra: 0, error: 0, skipped: 0, unverified: 0 };
const pageSize = 100;
const language = 'en';

export default function App() {
  const t: Text = copy;
  const [initialPreferences] = useState(() => readPreferences(localStorage));
  const [preferences, setPreferences] = useState(initialPreferences.value);
  const [source, setSource] = useState('');
  const [backup, setBackup] = useState('');
  const [scanning, setScanning] = useState(false);
  const [picking, setPicking] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [rows, setRows] = useState<Row[]>([]);
  const [search, setSearch] = useState('');
  const [filter, setFilter] = useState<Status | 'all' | 'problems'>('all');
  const [grouped, setGrouped] = useState(false);
  const [page, setPage] = useState(0);
  const [error, setError] = useState<keyof Text | null>(initialPreferences.error ? 'settingsReadFailed' : null);
  const [notice, setNotice] = useState('');
  const [view, setView] = useState<ViewContext | null>(null);
  const [toolBusy, setToolBusy] = useState(false);
  const [queueRunning, setQueueRunning] = useState(false);
  const [queueResults, setQueueResults] = useState<QueueResult[]>([]);
  const [queueLabel, setQueueLabel] = useState('');
  const queueStop = useRef(false);
  const [maximized, setMaximized] = useState(false);
  const [exportFormat, setExportFormat] = useState<'csv' | 'json' | 'html'>('csv');
  const [exporting, setExporting] = useState(false);
  const [exportedPath, setExportedPath] = useState('');
  const [selectedRow, setSelectedRow] = useState<Row | null>(null);
  const [elapsedMs, setElapsedMs] = useState(0);
  const scanStarted = useRef<number | null>(null);
  const readSamples = useRef<{ phase: string; samples: ReadSample[] }>({ phase: '', samples: [] });
  const dialog = useRef<HTMLDialogElement>(null);
  const pending = useRef<{ rows: Row[]; progress: Progress | null; dirty: boolean }>({ rows: [], progress: null, dirty: false });
  const busy = useRef(false);
  const generation = useRef(0);
  const deferredSearch = useDeferredValue(search);
  const number = (n: number) => new Intl.NumberFormat(language).format(n);

  useEffect(() => {
    if (!isTauri()) return;
    const appWindow = getCurrentWindow();
    let mounted = true;
    const update = () => { void appWindow.isMaximized().then(value => { if (mounted) setMaximized(value); }).catch(() => {}); };
    update();
    const listener = appWindow.onResized(update);
    return () => { mounted = false; void listener.then(unlisten => unlisten()).catch(() => {}); };
  }, []);

  function publish() {
    if (!pending.current.dirty) return;
    setRows([...pending.current.rows]);
    setProgress(pending.current.progress);
    pending.current.dirty = false;
  }

  useEffect(() => {
    const timer = window.setInterval(publish, 150);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!scanning) return;
    const timer = window.setInterval(() => {
      const now = performance.now();
      const latest = pending.current.progress;
      if (latest) {
        if (readSamples.current.phase !== latest.phase) readSamples.current = { phase: latest.phase, samples: [] };
        if (latest.phase === 'hashing') {
          readSamples.current.samples = [...readSamples.current.samples.filter(sample => sample.at >= now - 8500), { at: now, bytes: latest.bytesRead }];
        }
      }
      setElapsedMs(now - (scanStarted.current ?? now));
    }, 500);
    return () => window.clearInterval(timer);
  }, [scanning]);

  const filtered = useMemo(() => {
    const query = deferredSearch.toLocaleLowerCase(language);
    const found = rows.filter(row => (filter === 'all' || (filter === 'problems' ? row.status !== 'matched' : row.status === filter)) && row.path.toLocaleLowerCase(language).includes(query));
    return grouped ? found.sort((a, b) => folderOf(a).localeCompare(folderOf(b), 'en', { numeric: true }) || a.path.localeCompare(b.path, 'en', { numeric: true })) : found;
  }, [rows, deferredSearch, filter, grouped]);
  const pages = Math.max(1, Math.ceil(filtered.length / pageSize));
  const currentPage = Math.min(page, pages - 1);
  const visible = filtered.slice(currentPage * pageSize, (currentPage + 1) * pageSize);
  const counts = report?.counts ?? progress?.counts ?? emptyCounts;
  const hasIssues = counts.error + counts.skipped + counts.unverified > 0;
  const hasDifferences = counts.different + counts.missing + counts.extra > 0;
  const heading = scanning ? t[progress?.phase === 'checking' ? 'checking' : progress?.phase === 'hashing' ? 'hashing' : 'indexing'] : report ? (report.cancelled ? t.cancelled : view?.mode === 'manifest' ? t.manifestVerified : t.done) : t.ready;
  const hint = queueLabel || (view?.historical ? t.historicalHint : scanning ? t.scanningHint : report ? report.cancelled ? t.partialHint : hasIssues ? t.issuesHint : hasDifferences ? t.differencesHint : view?.scope.exclusions.length ? t.scoped : counts.matched ? t.identicalHint : t.emptyHint : t.readyHint);
  const percent = report && !report.cancelled ? 100 : progress?.totalBytes ? Math.min(100, progress.bytesRead / progress.totalBytes * 100) : progress?.total ? progress.processed / progress.total * 100 : 0;
  const indeterminate = scanning && (!progress || progress.phase === 'indexing');
  const estimate = estimateRead(readSamples.current.samples, progress?.totalBytes ?? 0, progress?.phase ?? '', performance.now());
  const speed = scanning ? estimate.bytesPerSecond : report && report.durationMs > 0 ? report.bytesRead / (report.durationMs / 1000) : null;

  function clearComparison() {
    setReport(null); setRows([]); setProgress(null); setPage(0); setSearch(''); setFilter('all'); setSelectedRow(null);
    setElapsedMs(0); setError(null); setExportedPath(''); setNotice(''); setView(null);
    pending.current = { rows: [], progress: null, dirty: false };
    readSamples.current = { phase: '', samples: [] };
  }

  function folderOf(row: Row) { return row.path.replace(/\\/g, '/').split('/').slice(0, -1).join('/') || '.'; }
  function statusText(status: Status) { return view?.mode === 'manifest' && status === 'missing' ? t.missingFromFolder : view?.mode === 'manifest' && status === 'extra' ? t.extraInFolder : t[status]; }
  function loadSnapshot(snapshot: Snapshot, historical: boolean) {
    clearComparison(); setSource(snapshot.source); setBackup(snapshot.mode === 'folders' ? snapshot.backup : '');
    setReport(snapshot.summary); setRows(snapshot.rows);
    const { rows: loadedRows, summary: _summary, ...context } = snapshot;
    setView({ ...context, historical }); pending.current = { rows: loadedRows, progress: null, dirty: false };
  }
  async function loadHistory(id: string) { loadSnapshot(await invoke<Snapshot>('load_report', { id, path: null, activate: true }), true); }
  async function notify(body: string) {
    if (!preferences.notify) return;
    try { await sendNotification({ title: 'MirrorProof', body }); } catch { setNotice(t.notificationFailed); }
  }

  async function windowAction(action: 'minimize' | 'toggleMaximize' | 'close') {
    if (!isTauri()) { setError('desktopOnly'); return; }
    try { await getCurrentWindow()[action](); } catch { setError('windowActionFailed'); }
  }

  async function chooseFolder(side: 'source' | 'backup') {
    if (!isTauri()) { setError('desktopOnly'); return; }
    setPicking(true); setError(null); setExportedPath('');
    try {
      const folder = await open({ directory: true, multiple: false, title: t[side], defaultPath: (side === 'source' ? source : backup) || undefined });
      if (typeof folder === 'string') {
        (side === 'source' ? setSource : setBackup)(folder);
        clearComparison();
      }
    } catch { setError('pickerFailed'); } finally { setPicking(false); }
  }

  async function runOne(a: string, b: string): Promise<{ summary?: Report; error?: string }> {
    if (busy.current) return { error: 'busy' };
    if (!isTauri()) { setError('desktopOnly'); return { error: 'desktopOnly' }; }
    busy.current = true;
    const id = ++generation.current;
    clearComparison();
    setSource(a); setBackup(b);
    setView({ source: a, backup: b, mode: 'folders', scope: { exclusions: preferences.exclusions }, startedAtUnixSeconds: Math.floor(Date.now() / 1000), finishedAtUnixSeconds: Math.floor(Date.now() / 1000), historical: false });
    scanStarted.current = performance.now();
    setScanning(true); setCancelling(false);
    const onProgress = new Channel<Progress>();
    onProgress.onmessage = message => {
      if (generation.current !== id) return;
      pending.current.rows.push(...message.rows);
      pending.current.progress = { ...message, rows: [] };
      pending.current.dirty = true;
    };
    try {
      const result = await invoke<Report>('scan', { source: a, backup: b, scope: { exclusions: preferences.exclusions }, onProgress });
      publish(); setReport(result);
      if (result.historyError) setNotice(`${t.historySaveFailed}${result.historyError === 'historyInsideScan' ? ` ${t.historyInsideScan}` : ''}`);
      return { summary: result };
    } catch (cause) {
      publish();
      const key = String(cause);
      setError(Object.hasOwn(copy, key) ? key as keyof Text : 'scanFailed');
      return { error: key };
    } finally { busy.current = false; setScanning(false); setCancelling(false); }
  }

  async function start() {
    const result = await runOne(source, backup);
    await notify(result.error ? t.scanFailed : result.summary?.cancelled ? t.cancelled : `${t.done} · ${result.summary?.processed} ${t.resultsUnit}`);
  }
  async function runQueue(pairs: SavedPair[]) {
    if (busy.current || queueRunning) return;
    queueStop.current = false; setQueueRunning(true); setQueueResults([]);
    const outcomes: QueueResult[] = [];
    try {
      for (let i = 0; i < pairs.length; i++) {
        if (queueStop.current) break;
        const pair = pairs[i]; setQueueLabel(`${i + 1}/${pairs.length} · ${pair.name}`);
        const result = await runOne(pair.source, pair.backup); outcomes.push({ pair, ...result }); setQueueResults([...outcomes]);
        if (result.summary?.cancelled) break;
      }
    } finally { setQueueRunning(false); setQueueLabel(''); }
    await notify(`${queueStop.current ? t.queueStopped : t.queueFinished} · ${outcomes.length}/${pairs.length}`);
  }

  async function cancel() {
    queueStop.current = true;
    setCancelling(true);
    try { await invoke('cancel_scan'); } catch { setError('cancelFailed'); setCancelling(false); }
  }

  async function exportResults() {
    if (!report || scanning || exporting) return;
    setExporting(true); setError(null); setExportedPath('');
    try {
      const path = await save({ title: t.exportTitle, defaultPath: `MirrorProof-${new Date().toISOString().replace(/[:.]/g, '-')}.${exportFormat}`, filters: [{ name: exportFormat === 'csv' ? 'CSV (Excel)' : exportFormat.toUpperCase(), extensions: [exportFormat] }] });
      if (!path) return;
      await invoke('export_report', { path, format: exportFormat });
      setExportedPath(path);
    } catch (cause) {
      const key = String(cause);
      setError(Object.hasOwn(copy, key) ? key as keyof Text : 'exportFailed');
    } finally { setExporting(false); }
  }

  function detail(row: Row) {
    const side = row.side === 'both' ? '' : `${t[row.side]}: `;
    return `${side}${t[row.reason]}${row.osCode !== null ? ` (${t.osError} ${row.osCode})` : ''}`;
  }

  return <div className={`app-shell ${maximized ? 'is-maximized' : ''}`}>
    <header className="app-header">
      <div className="titlebar-drag" data-tauri-drag-region>
        <img className="brand-app-icon" src={appMark} alt="" draggable={false} />
        <img className="brand-wordmark" src={wordmark} alt="MirrorProof" draggable={false} />
        <span className="brand-description">{t.subtitle}</span>
      </div>
      <div className="header-actions">
        <button className="icon-button" aria-label={t.about} title={t.about} onClick={() => dialog.current?.showModal()}><Info size={20} /></button>
      </div>
      <div className="window-controls" aria-label={t.windowControls}>
        <button aria-label={t.minimize} title={t.minimize} onClick={() => void windowAction('minimize')}><Minus size={16} /></button>
        <button aria-label={maximized ? t.restore : t.maximize} title={maximized ? t.restore : t.maximize} onClick={() => void windowAction('toggleMaximize')}>{maximized ? <Copy size={13} /> : <Square size={13} />}</button>
        <button className="window-close" aria-label={t.closeWindow} title={t.closeWindow} onClick={() => void windowAction('close')}><X size={17} /></button>
      </div>
    </header>

    <main>
      <div className="workspace-heading"><h1>{t.compareTitle}</h1><div className="workspace-actions"><SavedPairs source={source} backup={backup} disabled={scanning || picking || exporting || toolBusy || queueRunning} onLoad={pair => { setSource(pair.source); setBackup(pair.backup); clearComparison(); }} /><HistoryDialog disabled={scanning || picking || exporting || toolBusy || queueRunning} onLoad={snapshot => loadSnapshot(snapshot, true)} /><ToolsDialog disabled={scanning || picking || exporting || toolBusy || queueRunning} source={source} preferences={preferences} queueResults={queueResults} onPreferences={value => { localStorage.setItem(preferenceKey, JSON.stringify(value)); setPreferences(value); }} onQueue={pairs => { void runQueue(pairs); }} onSnapshot={snapshot => loadSnapshot(snapshot, false)} onHistory={loadHistory} onBusy={setToolBusy} /></div></div>
      <section className="folder-pair" aria-label={`${t.source} / ${t.backup}`}>
        {(['source', 'backup'] as const).map(side => {
          const manifestReference = side === 'backup' && view?.mode === 'manifest';
          const value = side === 'source' ? source : manifestReference ? view.backup : backup;
          const name = value.split(/[\\/]/).filter(Boolean).at(-1) || value;
          return <div className={`folder-card ${value ? 'selected' : ''}`} key={side}>
            <div className="folder-label"><span className={`folder-icon ${side}`}>{side === 'source' ? <FolderInput size={24} strokeWidth={1.65} /> : <FolderCheck size={24} strokeWidth={1.65} />}</span><div><h2>{manifestReference ? t.manifestReference : t[side]}</h2><span className="folder-hint">{manifestReference ? t.manifestReferenceHint : t[side === 'source' ? 'sourceHint' : 'backupHint']}</span></div></div>
            <button className="button secondary folder-button" onClick={() => void chooseFolder(side)} disabled={scanning || picking || exporting || toolBusy || queueRunning} aria-label={`${t[value ? 'change' : 'choose']}: ${t[side]}`}><FolderOpen size={16} />{t[value ? 'change' : 'choose']}</button>
            <div className={`folder-path ${value ? '' : 'unset'}`} title={value}><strong>{value ? name : t.noFolder}</strong><span>{value || t.folderNotSelected}</span>{value && <Check size={15} aria-hidden="true" />}</div>
          </div>;
        })}
      </section>

      <section className={`scan-panel ${scanning ? 'active' : ''}`} aria-labelledby="scan-heading">
        <div className="scan-main"><div className="scan-copy"><h2 id="scan-heading" aria-live="polite"><ScanLine size={18} />{heading}</h2><p title={hint}>{hint}</p></div>
          {scanning || queueRunning ? <button className="button secondary cancel-button" onClick={() => void cancel()} disabled={cancelling}><Square size={13} />{cancelling ? t.cancelling : t.cancel}</button> : <button className="button primary" onClick={() => void start()} disabled={!source || !backup || picking || exporting || toolBusy}><Play size={15} fill="currentColor" />{report && view?.mode !== 'manifest' ? t.again : t.scan}</button>}
        </div>
        <div className="scan-details">
          <div className="progress-track-row"><div className={`progress-track ${indeterminate ? 'indeterminate' : ''}`} role="progressbar" aria-label={heading} aria-valuemin={0} aria-valuemax={100} aria-valuenow={indeterminate ? undefined : Math.round(percent)}><div style={{ width: `${percent}%` }} /></div><span className="progress-percent">{indeterminate ? '…' : `${number(Math.round(percent))}%`}</span></div>
          <div className="progress-line"><span className="current-file" title={progress?.currentPath}>{scanning && progress?.currentPath ? progress.currentPath : progress?.phase === 'indexing' ? `${number(progress.indexed)} ${t.indexed}` : `${number(report?.processed ?? progress?.processed ?? 0)} / ${number(report?.total ?? progress?.total ?? 0)} ${t.processed}`}</span><span className="scan-metrics"><span>{bytes(report?.bytesRead ?? progress?.bytesRead ?? 0)} {t.read}</span>{(scanning || report) && <span>{duration(report?.durationMs ?? elapsedMs)} {t.elapsed}</span>}{speed !== null && <span title={scanning ? t.speedHint : t.averageSpeed}>{bytes(speed)}/s</span>}{scanning && progress?.phase === 'hashing' && <span className="scan-eta">{estimate.remainingMs === null ? t.estimating : `${t.approximately} ${duration(Math.max(1000, estimate.remainingMs))} ${t.remaining}`}</span>}</span></div>
        </div>
      </section>

      <section className="results-section" aria-labelledby="results-heading">
        <div className="section-heading"><h2 id="results-heading" title={view?.mode === 'manifest' ? view.backup : undefined}>{view?.mode === 'manifest' ? t.manifestResults : t.results}</h2><div className="export-controls"><span className="results-count" title={view?.historical ? `${t.historicalHint} ${new Date(view.finishedAtUnixSeconds * 1000).toLocaleString('en')}` : undefined}>{view?.historical ? `${t.historical} · ${new Date(view.finishedAtUnixSeconds * 1000).toLocaleDateString('en')}` : report?.cancelled ? t.cancelled : rows.length ? `${number(rows.length)} ${t.resultsUnit}` : scanning ? t.indexing : t.noScan}</span><label className="sr-only" htmlFor="export-format">{t.exportFormat}</label><select id="export-format" value={exportFormat} onChange={event => setExportFormat(event.target.value as 'csv' | 'json' | 'html')} disabled={!report || scanning || exporting || queueRunning || toolBusy}><option value="csv">CSV (Excel)</option><option value="json">JSON</option><option value="html">HTML</option></select><button className="button export-button" onClick={() => void exportResults()} disabled={!report || scanning || exporting || queueRunning || toolBusy} title={t.exportAllHint}>{exporting ? <LoaderCircle size={15} className="spin" /> : <FileOutput size={15} />}{exporting ? t.exporting : t.exportAll}</button></div></div>
        {exportedPath && <div className="export-notice" role="status"><CircleCheck size={16} /><span>{t.exportSaved} <strong title={exportedPath}>{exportedPath.split(/[\\/]/).at(-1)}</strong></span></div>}
        {error && <div className="error-banner" role="alert"><CircleAlert size={17} /><span>{t[error]}</span><button className="icon-button" aria-label={t.close} onClick={() => setError(null)}><X size={16} /></button></div>}
        {notice && <div className="scan-notice" role="status"><span>{notice}</span><button className="icon-button" aria-label={t.close} onClick={() => setNotice('')}><X size={15} /></button></div>}
        <div className="results-surface">
          <div className="summary-strip">{(['matched', 'different', 'missing', 'extra'] as const).map(status => <button className={`summary-item ${status} ${filter === status ? 'chosen' : ''}`} key={status} aria-pressed={filter === status} onClick={() => { setFilter(filter === status ? 'all' : status); setPage(0); }}><span className={`summary-symbol ${status}`}>{status === 'matched' ? <FileCheck size={18} /> : status === 'different' ? <FileDiff size={18} /> : status === 'missing' ? <FileMinus size={18} /> : <FilePlus size={18} />}</span><span>{statusText(status)}</span><strong>{number(counts[status])}</strong></button>)}</div>
          <div className="toolbar"><div className="search-field"><Search size={17} /><input type="search" aria-label={t.search} placeholder={t.search} value={search} onChange={event => { setSearch(event.target.value); setPage(0); }} /></div><label className="group-toggle"><input type="checkbox" checked={grouped} onChange={event => { setGrouped(event.target.checked); setPage(0); }} />{t.groupFolders}</label><label className="sr-only" htmlFor="status-filter">{t.filter}</label><select id="status-filter" value={filter} onChange={event => { setFilter(event.target.value as Status | 'all' | 'problems'); setPage(0); }}><option value="all">{t.all}</option><option value="problems">{t.problems}</option>{statuses.map(status => <option key={status} value={status}>{statusText(status)} ({number(counts[status])})</option>)}</select></div>
          {visible.length ? <div className="table-scroll" tabIndex={0} aria-label={t.results}><table><thead><tr><th scope="col">{t.file}</th><th scope="col">{t.status}</th><th scope="col" className="size-column">{t.sourceSize}</th><th scope="col" className="size-column">{view?.mode === 'manifest' ? t.referenceSize : t.backupSize}</th></tr></thead><tbody>{visible.map((row, index) => <Fragment key={`${currentPage}-${index}`}>{grouped && (index === 0 || folderOf(row) !== folderOf(visible[index - 1])) && <tr className="folder-group"><th colSpan={4} scope="rowgroup">{folderOf(row)}</th></tr>}<tr onClick={() => setSelectedRow(row)}><td><button className="file-name file-detail-button" onClick={event => { event.stopPropagation(); setSelectedRow(row); }} aria-label={`${t.viewDetails}: ${row.path}`}><File size={16} aria-hidden="true" /><span title={row.path}>{row.path}</span></button><p className="row-detail">{detail(row)}</p></td><td><span className={`status-badge ${row.status}`}>{row.status === 'matched' ? <Check size={12} /> : <span className={`status-dot ${row.status}`} />}{statusText(row.status)}</span></td><td className="size-column">{bytes(row.sourceSize)}</td><td className="size-column">{bytes(row.backupSize)}</td></tr></Fragment>)}</tbody></table></div> : <div className="empty-results"><span className="empty-mark" aria-hidden="true"><FolderCheck size={32} strokeWidth={1.4} /></span><h3>{rows.length || report ? t.noResults : scanning ? t.waitingTitle : t.emptyTitle}</h3><p>{rows.length || report ? t.noResultsBody : scanning ? t.waitingBody : t.emptyBody}</p></div>}
          <div className="pagination"><span>{t.showing} {number(filtered.length ? currentPage * pageSize + 1 : 0)}–{number(Math.min((currentPage + 1) * pageSize, filtered.length))} {t.of} {number(filtered.length)}</span><div><button className="icon-button" aria-label={t.previous} onClick={() => setPage(Math.max(0, currentPage - 1))} disabled={currentPage === 0}><ChevronLeft size={17} /></button><span>{t.page} {number(currentPage + 1)} {t.of} {number(pages)}</span><button className="icon-button" aria-label={t.next} onClick={() => setPage(currentPage + 1)} disabled={currentPage + 1 >= pages}><ChevronRight size={17} /></button></div></div>
        </div>
      </section>
    </main>

    {selectedRow && <FileDetails row={selectedRow} description={detail(selectedRow)} source={view?.source ?? source} backup={view?.backup ?? backup} mode={view?.mode ?? 'folders'} onClose={() => setSelectedRow(null)} />}
    <dialog ref={dialog} aria-labelledby="about-title" className="about-dialog">
      <div className="dialog-heading"><h2 id="about-title">{t.about}</h2><button className="icon-button" aria-label={t.close} onClick={() => dialog.current?.close()} autoFocus><X size={20} /></button></div>
      <div className="about-product"><img src={appMark} alt="" /><div><strong>MirrorProof</strong><span>{t.prototype}</span></div></div>
      <p>{t.aboutBody}</p><details className="about-scope"><summary>{t.verificationScope}</summary><p>{t.limits}</p></details>
      <DeveloperCard />
      <small>{t.license}</small>
    </dialog>
  </div>;
}
