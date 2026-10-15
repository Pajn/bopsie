import { settlesOpen } from './settle';

describe('settlesOpen', () => {
  const travel = 600;

  it('settles where the panel is let go of, without a flick', () => {
    expect(settlesOpen(0.6, 0, travel)).toBe(true);
    expect(settlesOpen(0.4, 0, travel)).toBe(false);
  });

  it('closes on a flick down from nearly open', () => {
    expect(settlesOpen(0.9, 3_000, travel)).toBe(false);
  });

  it('opens on a flick up from nearly closed', () => {
    expect(settlesOpen(0.1, -3_000, travel)).toBe(true);
  });

  it('stays open for a slow drag that is let go of early', () => {
    expect(settlesOpen(0.85, 200, travel)).toBe(true);
  });
});
