import type { Text } from './copy';

export type Status = 'matched' | 'different' | 'missing' | 'extra' | 'error' | 'skipped' | 'unverified';
export type Counts = Record<Status, number>;
export type Row = { path: string; status: Status; reason: keyof Text; side: 'source' | 'backup' | 'both'; sourceSize: number | null; backupSize: number | null; osCode: number | null };
export type Progress = { phase: 'indexing' | 'hashing' | 'checking' | 'done' | 'cancelled'; indexed: number; processed: number; total: number; bytesRead: number; totalBytes: number; currentPath: string; counts: Counts; rows: Row[] };
export type Report = { cancelled: boolean; processed: number; total: number; bytesRead: number; durationMs: number; counts: Counts; historyError?: string | null; historyId?: string | null };
export type Scope = { exclusions: string[] };
export type Snapshot = { source: string; backup: string; startedAtUnixSeconds: number; finishedAtUnixSeconds: number; summary: Report; rows: Row[]; scope: Scope; mode: 'folders' | 'manifest'; baselineComplete?: boolean | null };
export type ViewContext = Omit<Snapshot, 'rows' | 'summary'> & { historical: boolean };
export type HistoryEntry = { id: string; source: string; backup: string; started: number; finished: number; summary: Report; scope: Scope; mode: string; error: string | null };
