export function bytes(value: number | null): string {
  if (value === null) return '—';
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  const index = Math.min(4, Math.floor(Math.log2(Math.max(1, value)) / 10));
  return `${new Intl.NumberFormat('en', { maximumFractionDigits: index ? 1 : 0 }).format(value / 1024 ** index)} ${units[index]}`;
}

export function duration(milliseconds: number): string {
  const seconds = Math.max(0, Math.floor(milliseconds / 1000));
  const minutes = Math.floor(seconds / 60);
  if (minutes >= 60) return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
  return minutes ? `${minutes}m ${seconds % 60}s` : `${seconds}s`;
}

export function comparedPath(root: string, relative: string): string {
  return relative && relative !== '.' ? `${root.replace(/[\\/]+$/, '')}\\${relative.replace(/\//g, '\\')}` : root;
}
