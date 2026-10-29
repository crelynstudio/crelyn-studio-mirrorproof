import type { Snapshot, Row } from './scanTypes';
export type ResultChange = { path: string; kind: 'added' | 'removed' | 'resolved' | 'issue' | 'changed'; before?: Row; after?: Row };
const rootKey = (path: string) => path.replace(/^\\\\\?\\/, '').replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
export function resultChanges(before: Snapshot, after: Snapshot): ResultChange[] {
  if (rootKey(before.source) !== rootKey(after.source) || rootKey(before.backup) !== rootKey(after.backup) || before.mode !== after.mode || JSON.stringify([...before.scope.exclusions].sort()) !== JSON.stringify([...after.scope.exclusions].sort())) throw new Error('differentScope');
  const key = (r: Row) => `${r.path}\0${r.side}`;
  const a = new Map(before.rows.map(r => [key(r), r])), b = new Map(after.rows.map(r => [key(r), r]));
  const changes: ResultChange[] = [];
  for (const id of new Set([...a.keys(), ...b.keys()])) {
    const old = a.get(id), current = b.get(id);
    if (old && current && JSON.stringify(old) === JSON.stringify(current)) continue;
    const kind = !old ? 'added' : !current ? 'removed' : old.status !== 'matched' && current.status === 'matched' ? 'resolved' : old.status === 'matched' && current.status !== 'matched' ? 'issue' : 'changed';
    changes.push({ path: (current ?? old)!.path, kind, before: old, after: current });
  }
  return changes;
}
