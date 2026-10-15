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
