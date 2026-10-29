import { useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { Clock3, FileInput, GitCompareArrows, X } from 'lucide-react';
import { copy as t } from './copy';
import type { HistoryEntry, Snapshot } from './scanTypes';
import { resultChanges } from './resultChanges';
import type { ResultChange } from './resultChanges';

export default function HistoryDialog({ disabled, onLoad }: { disabled: boolean; onLoad: (s: Snapshot) => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [entries, setEntries] = useState<HistoryEntry[]>([]), [selected, setSelected] = useState<string[]>([]);
  const [loading, setLoading] = useState(false), [error, setError] = useState('');
  const [changes, setChanges] = useState<ResultChange[] | null>(null), [query, setQuery] = useState('');
  const [dates, setDates] = useState('');
  const date = (seconds: number) => new Date(seconds * 1000).toLocaleString('en');
  async function show() {
    setLoading(true); setError(''); setChanges(null); setSelected([]); setQuery(''); dialog.current?.showModal();
    try { setEntries(await invoke<HistoryEntry[]>('list_history')); } catch { setError(t.storageFailed); } finally { setLoading(false); }
  }
  async function load(id?: string) {
    setLoading(true); setError('');
    try {
      const path = id ? null : await open({ multiple: false, filters: [{ name: 'MirrorProof JSON report', extensions: ['json'] }] });
      if (!id && typeof path !== 'string') return;
      const snapshot = await invoke<Snapshot>('load_report', { id: id ?? null, path, activate: true });
      onLoad(snapshot); dialog.current?.close();
    } catch { setError(t.invalidDocument); } finally { setLoading(false); }
  }
  async function compare() {
    setLoading(true); setError(''); setChanges(null);
    try {
      const sorted = entries.filter(e => selected.includes(e.id)).sort((a, b) => a.finished - b.finished || a.id.localeCompare(b.id));
      const before = await invoke<Snapshot>('load_report', { id: sorted[0].id, path: null, activate: false });
      const after = await invoke<Snapshot>('load_report', { id: sorted[1].id, path: null, activate: false });
      setChanges(resultChanges(before, after)); setDates(`${date(sorted[0].finished)} → ${date(sorted[1].finished)}`);
    } catch (cause) { setError(String(cause).includes('differentScope') ? t.differentScope : t.invalidDocument); } finally { setLoading(false); }
  }
  return <>
    <button className="button secondary heading-button" onClick={() => void show()} disabled={disabled}><Clock3 size={14} />{t.history}</button>
    <dialog ref={dialog} className="history-dialog" aria-labelledby="history-title">
      <div className="dialog-heading"><h2 id="history-title">{t.history}</h2><button className="icon-button" aria-label={t.close} onClick={() => dialog.current?.close()} autoFocus><X size={20} /></button></div>
      <div className="history-actions"><button className="button secondary" disabled={loading} onClick={() => void load()}><FileInput size={14} />{t.openReport}</button><button className="button secondary" disabled={loading || selected.length !== 2} onClick={() => void compare()}><GitCompareArrows size={14} />{t.compareResults}</button></div>
      <p className="dialog-help">{t.historyHint}</p>
      {error && <p className="dialog-error" role="alert">{error}</p>}
      {loading && <p className="dialog-help" role="status">{t.loading}</p>}
      {changes !== null ? <section className="changes-panel"><h3>{t.recordedChanges} ({changes.length})</h3><p className="dialog-help">{dates}<br />{t.changesHint}</p><input className="dialog-search" type="search" aria-label={t.searchChanges} placeholder={t.searchChanges} value={query} onChange={e => setQuery(e.target.value)} /><div className="change-list">{changes.filter(c => c.path.toLowerCase().includes(query.toLowerCase())).slice(0, 200).map((c, i) => <div className="change-item" key={i}><strong>{t[`change_${c.kind}`]}</strong><span>{c.path || '.'}</span><small>{c.before ? t[c.before.status] : '—'} → {c.after ? t[c.after.status] : '—'}</small></div>)}</div><p className="dialog-help">{t.first200}</p><button className="button secondary" onClick={() => setChanges(null)}>{t.backToHistory}</button></section> : <ul className="history-list">{entries.map(entry => <li key={entry.id} data-report-id={entry.id}><input type="checkbox" aria-label={`${t.selectReport}: ${date(entry.finished)}`} checked={selected.includes(entry.id)} disabled={loading || !!entry.error || (selected.length === 2 && !selected.includes(entry.id))} onChange={e => setSelected(e.target.checked ? [...selected, entry.id] : selected.filter(id => id !== entry.id))} /><div><strong>{date(entry.finished)}</strong><p title={entry.source}>{entry.source}</p><p title={entry.backup}>{entry.backup}</p><small>{entry.error ? t.scanFailed : `${entry.summary.cancelled ? t.cancelled : t.done} · ${entry.summary.processed} ${t.resultsUnit} · ${t.matched}: ${entry.summary.counts.matched}${entry.scope.exclusions.length ? ` · ${t.scoped}` : ''}`}</small></div><button className="button secondary" disabled={loading || !!entry.error} onClick={() => void load(entry.id)}>{t.openResult}</button></li>)}</ul>}
      {!entries.length && !loading && !error && <p className="dialog-help">{t.noHistory}</p>}
    </dialog>
  </>;
}
