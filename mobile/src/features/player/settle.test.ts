import { dragged, settlesOpen, skipFor } from './settle';

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

describe('dragged', () => {
  it('opens as the finger goes up and closes as it comes down', () => {
    expect(dragged(0, -300, 600)).toBeCloseTo(0.5);
    expect(dragged(1, 300, 600)).toBeCloseTo(0.5);
  });

  it('stops dead at either end', () => {
    expect(dragged(0, 200, 600)).toBe(0);
    expect(dragged(1, -10_000, 600)).toBe(1);
  });

  it('stays put before it has been measured', () => {
    expect(dragged(0.4, -300, 0)).toBe(0.4);
  });
});

describe('skipFor', () => {
  it('skips to the next track for a swipe to the left, the previous for one to the right', () => {
    expect(skipFor(-150, 0, 400)).toBe('next');
    expect(skipFor(150, 0, 400)).toBe('previous');
  });

  it('skips for a short flick', () => {
    expect(skipFor(-30, -1200, 400)).toBe('next');
  });

  it('stays for a short, slow swipe, or a flick back the way it came', () => {
    expect(skipFor(-60, -100, 400)).toBeNull();
    expect(skipFor(-150 * 0.1, 1200, 400)).toBeNull();
  });
});
