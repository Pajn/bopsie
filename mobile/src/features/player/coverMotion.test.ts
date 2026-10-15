import { coverMotion } from './coverMotion';

const mini = { x: 16, y: 8, size: 40 };
const full = { x: 90, y: 300, size: 300 };

/** Where the artwork's box lands on screen for a motion. */
const landed = (m: ReturnType<typeof coverMotion>) => {
  const centre = {
    x: full.x + full.size / 2 + m.translateX,
    y: full.y + full.size / 2 + m.translateY,
  };
  const size = full.size * m.scale;
  return { x: centre.x - size / 2, y: centre.y - size / 2, size };
};

describe('coverMotion', () => {
  it('lands on the mini player docked and on the full player open', () => {
    const docked = landed(coverMotion(mini, full, 0, 6));
    expect(docked.x).toBeCloseTo(mini.x);
    expect(docked.y).toBeCloseTo(mini.y);
    expect(docked.size).toBeCloseTo(mini.size);
    const open = landed(coverMotion(mini, full, 1, 6));
    expect(open.x).toBeCloseTo(full.x);
    expect(open.y).toBeCloseTo(full.y);
    expect(open.size).toBeCloseTo(full.size);
  });

  it('keeps the corners the same size on screen as it scales', () => {
    for (const e of [0, 0.25, 0.5, 1]) {
      const m = coverMotion(mini, full, e, 6);
      expect(m.borderRadius * m.scale).toBeCloseTo(6);
    }
  });

  it('stays at the ends when the spring is asked past them', () => {
    expect(coverMotion(mini, full, 1.2, 6)).toEqual(coverMotion(mini, full, 1, 6));
    expect(coverMotion(mini, full, -0.1, 6)).toEqual(coverMotion(mini, full, 0, 6));
  });
});
