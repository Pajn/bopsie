/** Minutes in a week. */
const WEEK = 7 * 24 * 60;

export const WEEKDAYS = [
  'Monday',
  'Tuesday',
  'Wednesday',
  'Thursday',
  'Friday',
  'Saturday',
  'Sunday',
] as const;

/** A time of the week: an hour of a day, Monday being day 0. */
export interface WeekTime {
  hour: number;
  weekday: number;
}

/**
 * A local time of the week in UTC, or back, by the browser's `offset`: minutes UTC is ahead
 * of local time, as `Date.getTimezoneOffset` says. Crossing midnight moves the day.
 */
function shift({ hour, weekday }: WeekTime, minutes: number): WeekTime {
  const at = ((((weekday * 24 + hour) * 60 + minutes) % WEEK) + WEEK) % WEEK;
  return { weekday: Math.floor(at / (24 * 60)), hour: Math.round((at % (24 * 60)) / 60) % 24 };
}

export const toUtc = (local: WeekTime, offset: number) => shift(local, offset);
export const toLocal = (utc: WeekTime, offset: number) => shift(utc, -offset);

/** Bytes as people read them: `512 B`, `3.4 MB`, `1.2 GB`. */
export function formatBytes(bytes: number): string {
  const units = ['B', 'kB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[unit]}`;
}

const clock = (hour: number) => `${String(hour).padStart(2, '0')}:00`;

/** What a schedule says, in local time. */
export function scheduleLabel(
  schedule: { every: 'daily' | 'weekly'; hour: number; weekday: number; keep: number } | null,
  offset: number,
): string {
  if (!schedule) return 'Off';
  const local = toLocal(schedule, offset);
  const when =
    schedule.every === 'daily'
      ? `Daily at ${clock(local.hour)}`
      : `${WEEKDAYS[local.weekday]}s at ${clock(local.hour)}`;
  return `${when}, keeping ${schedule.keep}`;
}
