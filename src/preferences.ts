export type Preferences = { exclusions: string[]; notify: boolean };
export const preferenceKey = 'mirrorproof-preferences-v1';
export function validExclusions(rules: string[]): boolean {
  return rules.length <= 32 && rules.every(rule => rule.length > 0 && new TextEncoder().encode(rule).length <= 240 && !/^[\\/]|[\u0000-\u001f\u007f:<>"|]/.test(rule) && !rule.replace(/\\/g, '/').split('/').some(part => part === '..' || part === '.'));
}
export function readPreferences(storage: Pick<Storage, 'getItem'>): { value: Preferences; error: boolean } {
  try {
    const raw = storage.getItem(preferenceKey);
    if (raw === null) return { value: { exclusions: [], notify: false }, error: false };
    const v = JSON.parse(raw);
    if (!v || !Array.isArray(v.exclusions) || !v.exclusions.every((s: unknown) => typeof s === 'string') || !validExclusions(v.exclusions) || typeof v.notify !== 'boolean') throw new Error('Invalid preferences');
    return { value: { exclusions: v.exclusions, notify: v.notify }, error: false };
  } catch { return { value: { exclusions: [], notify: false }, error: true }; }
}
