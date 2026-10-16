import type { PlayerQueue } from '@bopsie/client/api/schemas/player';

/** Where a track's position was measured, and whether it moves on from there. */
export interface Measured {
  /** Seconds into the track at `measuredAt`. */
  elapsed: number;
  /** Unix seconds. */
  measuredAt: number;
  playing: boolean;
  /** Seconds, when known. */
  duration?: number | null | undefined;
}

/**
 * Seconds into a track at `nowMs`. The server reports the elapsed time with the moment it
 * measured it; while playing, the position moves on from there. Kept within the track's
 * duration when that is known.
 */
export function positionAt({ elapsed, measuredAt, playing, duration }: Measured, nowMs: number) {
  const position = playing ? elapsed + Math.max(0, nowMs / 1000 - measuredAt) : elapsed;
  return duration ? Math.min(position, duration) : position;
}

/**
 * Whether the queue's position moves on: playing, and heard already. While its speakers get
 * ready, the server says it is playing but buffering, and the position stands still.
 */
export function advancing(queue: Pick<PlayerQueue, 'state' | 'buffering'>): boolean {
  return queue.state === 'playing' && !queue.buffering;
}

/** Seconds into the queue's current item at `nowMs`; see `positionAt`. */
export function queuePosition(queue: PlayerQueue, nowMs: number): number {
  return positionAt(
    {
      elapsed: queue.elapsed_time,
      measuredAt: queue.elapsed_time_last_updated,
      playing: advancing(queue),
      duration: queue.current_item?.duration,
    },
    nowMs,
  );
}

/** `m:ss`, or `h:mm:ss` from an hour. */
export function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}
