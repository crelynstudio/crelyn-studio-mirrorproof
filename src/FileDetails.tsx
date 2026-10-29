import { useEffect, useRef, useState } from 'react';
import { Copy, X } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { copy as t } from './copy';
import { bytes, comparedPath } from './format';
import type { Row } from './scanTypes';

export default function FileDetails({ row, description, source, backup, mode, onClose }: {
  row: Row; description: string; source: string; backup: string; mode: 'folders' | 'manifest'; onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [message, setMessage] = useState('');
  const [copyError, setCopyError] = useState(false);
  useEffect(() => { dialog.current?.showModal(); }, []);

  async function copyPath(value: string) {
    try { await navigator.clipboard.writeText(value); setCopyError(false); setMessage(t.pathCopied); }
    catch { setCopyError(true); setMessage(t.clipboardFailed); }
  }
  async function reveal(side: string) {
    try { await invoke('reveal_file', { path: row.path, side }); setCopyError(false); setMessage(''); }
    catch { setCopyError(true); setMessage(t.revealFailed); }
  }

  return <dialog ref={dialog} className="file-dialog" aria-labelledby="file-detail-title" onClose={onClose}>
    <div className="dialog-heading"><h2 id="file-detail-title">{t.fileDetails}</h2><button className="icon-button" aria-label={t.close} onClick={() => dialog.current?.close()} autoFocus><X size={20} /></button></div>
    <p className="detail-relative">{row.path || '.'}</p>
    <div className={`detail-status ${row.status}`}><strong>{mode === 'manifest' && row.status === 'missing' ? t.missingFromFolder : mode === 'manifest' && row.status === 'extra' ? t.extraInFolder : t[row.status]}</strong><p>{description}</p></div>
    <div className="detail-folders">{([{ side: 'source', label: t.source, root: source, size: row.sourceSize, expected: row.status === (mode === 'manifest' ? 'missing' : 'extra') }, { side: 'backup', label: mode === 'manifest' ? t.manifestReference : t.backup, root: backup, size: row.backupSize, expected: mode !== 'manifest' && row.status === 'missing' }]).map(side => {
      const fullPath = mode === 'manifest' && side.side === 'backup' ? backup : comparedPath(side.root, row.path);
      return <section className="detail-folder" key={side.label} aria-label={side.label}>
        <h3>{side.label}</h3><span className="detail-path-label">{side.expected ? t.expectedPath : t.fullPath}</span><p className="detail-path">{fullPath}</p>
        <div className="detail-folder-actions"><span>{t.size}: <strong>{bytes(side.size)}</strong></span><button className="button secondary" onClick={() => void copyPath(fullPath)} aria-label={`${t.copyPath}: ${side.label}`}><Copy size={14} />{t.copyPath}</button></div>
        <button className="button secondary reveal-button" disabled={side.expected || row.status === 'skipped'} onClick={() => void reveal(side.side)} aria-label={`${t.revealFile}: ${side.label}`}>{t.revealFile}</button>
      </section>;
    })}</div>
    <p className={copyError ? 'dialog-error' : 'dialog-message'} role={copyError ? 'alert' : 'status'}>{message}</p>
  </dialog>;
}
