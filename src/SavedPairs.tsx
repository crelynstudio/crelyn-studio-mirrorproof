import { useRef, useState } from 'react';
import { Bookmark, Plus, Trash2, X } from 'lucide-react';
import { copy as t } from './copy';
import { pairLimit, readSavedPairs, writeSavedPairs } from './pairStorage';
import type { SavedPair } from './pairStorage';

export default function SavedPairs({ source, backup, disabled, onLoad }: {
  source: string; backup: string; disabled: boolean; onLoad: (pair: SavedPair) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [pairs, setPairs] = useState<SavedPair[]>([]);
  const [name, setName] = useState('');
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');

  function show() {
    setError(''); setMessage(''); setName('');
    try { setPairs(readSavedPairs(localStorage)); } catch { setError(t.pairsReadFailed); }
    dialog.current?.showModal();
  }

  function savePair(event: React.SubmitEvent<HTMLFormElement>) {
    event.preventDefault(); setError(''); setMessage('');
    const trimmed = name.trim();
    if (disabled || !source || !backup || !trimmed) return;
    try {
      const current = readSavedPairs(localStorage);
      if (current.some(pair => pair.name.toLowerCase() === trimmed.toLowerCase())) { setError(t.pairNameExists); return; }
      if (current.length >= pairLimit) { setError(t.pairLimitReached); return; }
      const next = [...current, { id: crypto.randomUUID(), name: trimmed, source, backup }];
      writeSavedPairs(localStorage, next); setPairs(next); setName(''); setMessage(t.pairSaved);
    } catch { setError(t.pairsWriteFailed); }
  }

  function removePair(id: string) {
    setError(''); setMessage('');
    try {
      const next = readSavedPairs(localStorage).filter(pair => pair.id !== id);
      writeSavedPairs(localStorage, next); setPairs(next); setMessage(t.pairRemoved);
    } catch { setError(t.pairsWriteFailed); }
  }

  return <>
    <button className="button secondary saved-pairs-button" onClick={show} disabled={disabled}><Bookmark size={14} />{t.savedPairs}</button>
    <dialog ref={dialog} className="pairs-dialog" aria-labelledby="pairs-title">
      <div className="dialog-heading"><h2 id="pairs-title">{t.savedPairs}</h2><button className="icon-button" aria-label={t.close} onClick={() => dialog.current?.close()} autoFocus><X size={20} /></button></div>
      <form className="save-pair-form" onSubmit={savePair}>
        <label htmlFor="pair-name">{t.saveCurrentPair}</label>
        {source && backup ? <div className="pair-current"><p><strong>{t.source}:</strong> {source}</p><p><strong>{t.backup}:</strong> {backup}</p></div> : <p className="pairs-help">{t.pairChooseFirst}</p>}
        <div className="pair-name-row"><input id="pair-name" maxLength={80} required placeholder={t.pairNamePlaceholder} value={name} onChange={event => setName(event.target.value)} aria-label={t.pairName} disabled={!source || !backup || disabled} /><button className="button export-button" type="submit" disabled={!source || !backup || !name.trim() || disabled}><Plus size={15} />{t.savePair}</button></div>
      </form>
      {error && <p className="dialog-error" role="alert">{error}</p>}
      <p className="dialog-message" role="status">{message}</p>
      {pairs.length ? <ul className="saved-pair-list">{pairs.map(pair => <li key={pair.id}>
        <div><h3>{pair.name}</h3><p title={pair.source}>{t.source}: {pair.source}</p><p title={pair.backup}>{t.backup}: {pair.backup}</p></div>
        <div className="pair-actions"><button className="button secondary" disabled={disabled} aria-label={`${t.loadPair}: ${pair.name}`} onClick={() => { if (disabled) return; onLoad(pair); dialog.current?.close(); }}>{t.loadPair}</button><button className="icon-button" disabled={disabled} aria-label={`${t.removePair}: ${pair.name}`} title={`${t.removePair}: ${pair.name}`} onClick={() => removePair(pair.id)}><Trash2 size={16} /></button></div>
      </li>)}</ul> : !error && <p className="pairs-help">{t.noSavedPairs}</p>}
    </dialog>
  </>;
}
