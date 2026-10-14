/** Seconds as `m:ss`, or `h:mm:ss` past an hour. */
export function duration(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const rest = String(s % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${rest}` : `${m}:${rest}`;
}

/** Where a queue is now: its elapsed time moved on by the wall clock while it plays. */
export function elapsedNow(
  queue: { elapsed_time: number; elapsed_time_last_updated: number; state: string },
  nowMs: number = Date.now(),
): number {
  if (queue.state !== 'playing') return queue.elapsed_time;
  return queue.elapsed_time + Math.max(0, nowMs / 1000 - queue.elapsed_time_last_updated);
}
