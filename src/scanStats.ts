export type ReadSample = { at: number; bytes: number };

export function estimateRead(samples: ReadSample[], totalBytes: number, phase: string, now: number) {
  const result: { bytesPerSecond: number | null; remainingMs: number | null } = { bytesPerSecond: null, remainingMs: null };
  if (phase !== 'hashing') return result;
  const recent = samples.filter(sample => sample.at >= now - 8000 && sample.at <= now);
  const first = recent[0], last = recent.at(-1);
  if (!first || !last || last.at - first.at < 1000 || last.bytes <= first.bytes) return result;
  const lastChange = recent.filter((sample, index) => index > 0 && sample.bytes > recent[index - 1].bytes).at(-1);
  if (!lastChange || now - lastChange.at > 3000) return result;
  result.bytesPerSecond = (last.bytes - first.bytes) / ((last.at - first.at) / 1000);
  if (last.at - first.at >= 3000 && totalBytes > last.bytes) {
    result.remainingMs = (totalBytes - last.bytes) / result.bytesPerSecond * 1000;
  }
  return result;
}
