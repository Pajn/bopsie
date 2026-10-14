import type { LogLine } from '@bopsie/client/api/schemas/admin';

export const LEVELS = ['ERROR', 'WARN', 'INFO', 'DEBUG', 'TRACE'] as const;
export type LevelName = (typeof LEVELS)[number];

/** Lines kept in the page, as many as the server keeps. */
export const KEPT = 2_000;

/** `existing` with `incoming` added, without repeats, the oldest dropped past `KEPT`. */
export function append(existing: LogLine[], incoming: LogLine[]): LogLine[] {
  const last = existing.at(-1)?.seq ?? 0;
  const merged = [...existing, ...incoming.filter((line) => line.seq > last)];
  return merged.length > KEPT ? merged.slice(merged.length - KEPT) : merged;
}

/** The message and its fields on one line, as the server prints them. */
export function lineText(line: LogLine): string {
  const fields = Object.entries(line.fields).map(([key, value]) => `${key}=${value}`);
  return [line.message, ...fields].join(' ');
}

/** Lines at `level` or more severe, containing `filter` anywhere. */
export function visible(lines: LogLine[], level: LevelName, filter: string): LogLine[] {
  const most = LEVELS.indexOf(level);
  const needle = filter.trim().toLowerCase();
  return lines.filter((line) => {
    const rank = LEVELS.indexOf(line.level as LevelName);
    if (rank === -1 || rank > most) return false;
    if (!needle) return true;
    return `${line.target} ${lineText(line)}`.toLowerCase().includes(needle);
  });
}
