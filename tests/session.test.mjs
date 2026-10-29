import test from 'node:test';
import assert from 'node:assert/strict';
import { bytes, duration, comparedPath } from '../src/format.ts';
import { estimateRead } from '../src/scanStats.ts';
import { pairsKey, pairLimit, readSavedPairs, writeSavedPairs } from '../src/pairStorage.ts';
import { preferenceKey, readPreferences, validExclusions } from '../src/preferences.ts';
import { resultChanges } from '../src/resultChanges.ts';

test('elapsed time and paths handle boundaries, drive roots and Unicode', () => {
  assert.equal(duration(999), '0s');
  assert.equal(duration(60000), '1m 0s');
  assert.equal(duration(3661000), '1h 1m');
  assert.equal(bytes(null), '—');
  assert.equal(bytes(0), '0 B');
  assert.equal(bytes(1536), '1.5 KiB');
  assert.equal(comparedPath('C:\\', 'nested/İçerik.txt'), 'C:\\nested\\İçerik.txt');
  assert.equal(comparedPath('D:\\Backup\\', '.'), 'D:\\Backup\\');
  assert.equal(comparedPath('D:\\Backup', ''), 'D:\\Backup');
});

test('estimates wait for measurable hashing and enough samples', () => {
  const samples = [{ at: 0, bytes: 0 }, { at: 1000, bytes: 1000 }, { at: 3000, bytes: 3000 }];
  assert.deepEqual(estimateRead(samples, 9000, 'hashing', 3000), { bytesPerSecond: 1000, remainingMs: 6000 });
  assert.deepEqual(estimateRead(samples.slice(0, 2), 9000, 'hashing', 1000), { bytesPerSecond: 1000, remainingMs: null });
  for (const phase of ['indexing', 'checking', 'done', 'cancelled']) {
    assert.deepEqual(estimateRead(samples, 9000, phase, 3000), { bytesPerSecond: null, remainingMs: null });
  }
  assert.equal(estimateRead(samples, 0, 'hashing', 3000).remainingMs, null);
  assert.equal(estimateRead(samples, 3000, 'hashing', 3000).remainingMs, null);
});

test('stalled, empty and expired samples never produce a countdown', () => {
  for (const samples of [[], [{ at: 0, bytes: 0 }], [{ at: 0, bytes: 0 }, { at: 3000, bytes: 0 }]]) {
    assert.deepEqual(estimateRead(samples, 9000, 'hashing', 3000), { bytesPerSecond: null, remainingMs: null });
  }
  const stalled = [{ at: 0, bytes: 0 }, { at: 1000, bytes: 1000 }, { at: 5000, bytes: 1000 }];
  assert.deepEqual(estimateRead(stalled, 9000, 'hashing', 5000), { bytesPerSecond: null, remainingMs: null });
  assert.equal(estimateRead(stalled, 9000, 'hashing', 14000).remainingMs, null);
});

function storage(initial = null) {
  let raw = initial;
  return { getItem: key => { assert.equal(key, pairsKey); return raw; }, setItem: (key, value) => { assert.equal(key, pairsKey); raw = value; } };
}
const pair = { id: 'pair-1', name: 'Photos backup', source: 'C:\\Photos', backup: 'D:\\Backup' };

test('saved pairs round trip locally and removal leaves the other pairs intact', () => {
  const local = storage();
  assert.deepEqual(readSavedPairs(local), []);
  writeSavedPairs(local, [pair, { ...pair, id: 'pair-2', name: 'Work backup' }]);
  assert.equal(readSavedPairs(local).length, 2);
  writeSavedPairs(local, readSavedPairs(local).filter(p => p.id !== 'pair-2'));
  assert.deepEqual(readSavedPairs(local), [pair]);
});

test('invalid or unavailable storage fails without silently overwriting data', () => {
  const invalid = ['{', '{}', '[null]', JSON.stringify([{ ...pair, source: 1 }]),
    JSON.stringify([{ ...pair, backup: 'D:\\bad\0name' }]), JSON.stringify([pair, pair]),
    JSON.stringify([pair, { ...pair, id: 'pair-2', name: ' PHOTOS BACKUP ' }]),
    JSON.stringify(Array.from({ length: pairLimit + 1 }, (_, i) => ({ ...pair, id: `${i}`, name: `${i}` })))];
  for (const raw of invalid) {
    const local = storage(raw);
    assert.throws(() => readSavedPairs(local));
    assert.equal(local.getItem(pairsKey), raw);
  }
  assert.throws(() => readSavedPairs({ getItem: () => { throw new Error('Unavailable'); } }));
  assert.throws(() => writeSavedPairs({ setItem: () => { throw new Error('Quota exceeded'); } }, [pair]));
});

test('options remain opt-in and reject unsafe or excessive exclusion rules', () => {
  assert.deepEqual(readPreferences({ getItem: () => null }), { value: { exclusions: [], notify: false }, error: false });
  const value = { exclusions: ['*.tmp', 'cache/', '**/build/**'], notify: true };
  assert.deepEqual(readPreferences({ getItem: key => { assert.equal(key, preferenceKey); return JSON.stringify(value); } }).value, value);
  assert.equal(validExclusions(['../secret']), false);
  assert.equal(validExclusions(['C:\\data']), false);
  assert.equal(validExclusions(Array(33).fill('*.tmp')), false);
  assert.equal(readPreferences({ getItem: () => '{broken' }).error, true);
  assert.equal(readPreferences({ getItem: () => JSON.stringify({ exclusions: [], notify: 'yes' }) }).error, true);
});

const row = (path, status, reason = status === 'matched' ? 'sha256Match' : 'hashMismatch') => ({ path, status, reason, side: 'both', sourceSize: 4, backupSize: 4, osCode: null });
const snapshot = rows => ({ source: 'C:\\Source', backup: 'D:\\Backup', mode: 'folders', scope: { exclusions: [] }, rows });
test('result comparison describes recorded outcomes without inferring deletion', () => {
  const before = snapshot([row('stable','matched'),row('fixed','different'),row('broken','matched'),row('not-in-next-scan','matched')]);
  const after = snapshot([row('stable','matched'),row('fixed','matched'),row('broken','different'),row('new-record','different')]);
  assert.deepEqual(resultChanges(before, after).map(c => c.kind), ['resolved','issue','removed','added']);
  assert.equal(resultChanges(snapshot([row('same','different')]), snapshot([row('same','different')])).length, 0);
  assert.throws(() => resultChanges(before, { ...after, source: 'C:\\Other' }), /differentScope/);
  assert.throws(() => resultChanges(before, { ...after, scope: { exclusions: ['*.tmp'] } }), /differentScope/);
  assert.equal(resultChanges(before, { ...before, source: '\\\\?\\C:\\SOURCE\\' }).length, 0);
});
