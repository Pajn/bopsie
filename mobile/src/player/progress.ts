import type { PlayerQueue } from '@/api/schemas/player';

/**
 * Seconds into the current item at `nowMs`. The server reports the elapsed
 * time with the moment it measured it; while playing, the position moves on
 * from there. Kept within the item's duration when that is known.
 */
export function queuePosition(queue: PlayerQueue, nowMs: number): number {
  const base = queue.elapsed_time;
  const position =
    queue.state === 'playing'
      ? base + Math.max(0, nowMs / 1000 - queue.elapsed_time_last_updated)
      : base;
  const duration = queue.current_item?.duration;
  return duration ? Math.min(position, duration) : position;
}

/** `m:ss`, or `h:mm:ss` from an hour. */
export function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}
