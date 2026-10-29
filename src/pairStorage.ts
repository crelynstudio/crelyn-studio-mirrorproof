export type SavedPair = { id: string; name: string; source: string; backup: string };
export const pairsKey = 'mirrorproof-saved-pairs-v1';
export const pairLimit = 20;

export function readSavedPairs(storage: Pick<Storage, 'getItem'>): SavedPair[] {
  const raw = storage.getItem(pairsKey);
  if (raw === null) return [];
  const data: unknown = JSON.parse(raw);
  if (!Array.isArray(data) || data.length > pairLimit) throw new Error('Invalid saved pairs');
  const validText = (value: unknown, limit: number): value is string => typeof value === 'string' && value.trim().length > 0 && value.length <= limit && !value.includes('\0');
  const ids = new Set<string>();
  const names = new Set<string>();
  return data.map((pair: unknown) => {
    if (!pair || typeof pair !== 'object' || !('id' in pair) || !('name' in pair) || !('source' in pair) || !('backup' in pair)
      || !validText(pair.id, 64) || !validText(pair.name, 80) || !validText(pair.source, 32768) || !validText(pair.backup, 32768)
      || ids.has(pair.id) || names.has(pair.name.trim().toLowerCase())) throw new Error('Invalid saved pair');
    ids.add(pair.id); names.add(pair.name.trim().toLowerCase());
    return { id: pair.id, name: pair.name.trim(), source: pair.source, backup: pair.backup };
  });
}

export function writeSavedPairs(storage: Pick<Storage, 'setItem'>, pairs: SavedPair[]) {
  storage.setItem(pairsKey, JSON.stringify(pairs));
}
