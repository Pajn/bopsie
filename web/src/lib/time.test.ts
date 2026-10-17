import { duration, elapsedNow, uptime } from './time';

test('durations read as a clock', () => {
  expect(duration(0)).toBe('0:00');
  expect(duration(65.7)).toBe('1:05');
  expect(duration(3723)).toBe('1:02:03');
});

test('a playing queue moves on with the clock, a paused one does not', () => {
  const queue = { elapsed_time: 10, elapsed_time_last_updated: 1000, state: 'playing' };
  expect(elapsedNow(queue, 1_004_000)).toBe(14);
  expect(elapsedNow({ ...queue, state: 'paused' }, 1_004_000)).toBe(10);
  // A book played at 1.5× moves on faster.
  expect(elapsedNow({ ...queue, playback_speed: 1.5 }, 1_004_000)).toBe(16);
});

test('an uptime reads in its two largest units', () => {
  expect(uptime(45)).toBe('45 s');
  expect(uptime(12 * 60 + 59)).toBe('12 min');
  expect(uptime(3 * 3600 + 5 * 60)).toBe('3 h 5 min');
  expect(uptime(7200)).toBe('2 h');
  expect(uptime(2 * 86_400 + 4 * 3600 + 59)).toBe('2 d 4 h');
});
