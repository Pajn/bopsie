/** How far a flick carries the panel past where the finger let go, in seconds of its speed. */
const FLICK_SECONDS = 0.15;

/**
 * Whether a panel let go of mid-drag settles open: where it would come to rest, carried on by
 * its speed, is past halfway. `progress` runs from 0 closed to 1 open; `velocityY` is the
 * finger's, in points a second, downward positive; `travel` is how far the panel moves.
 */
export function settlesOpen(progress: number, velocityY: number, travel: number): boolean {
  'worklet';
  const resting = progress - (velocityY * FLICK_SECONDS) / travel;
  return resting > 0.5;
}

/**
 * Where something dragged between closed and open sits, for a finger that took hold of it at
 * `from` and has moved `translationY`, upward negative, over `travel`. It stops dead at either
 * end, and stays put before it has been measured.
 */
export function dragged(from: number, translationY: number, travel: number): number {
  'worklet';
  if (travel <= 0) return from;
  return Math.min(1, Math.max(0, from - translationY / travel));
}
