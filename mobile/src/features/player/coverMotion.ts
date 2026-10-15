/** A square's place in the player sheet. */
export interface Square {
  x: number;
  y: number;
  size: number;
}

/**
 * Where the artwork drawn at `full` is carried to, `expansion` of the way from the mini
 * player's `mini` to the full player's `full`: a move and a scale about its centre, and a
 * corner radius that, scaled, stays `radius` on screen.
 */
export function coverMotion(mini: Square, full: Square, expansion: number, radius: number) {
  'worklet';
  const e = Math.min(1, Math.max(0, expansion));
  const fullCentre = { x: full.x + full.size / 2, y: full.y + full.size / 2 };
  const miniCentre = { x: mini.x + mini.size / 2, y: mini.y + mini.size / 2 };
  const scale = mini.size / full.size + (1 - mini.size / full.size) * e;
  return {
    translateX: (miniCentre.x - fullCentre.x) * (1 - e),
    translateY: (miniCentre.y - fullCentre.y) * (1 - e),
    scale,
    borderRadius: radius / scale,
  };
}
