import { duration, elapsedNow } from './time';

test('durations read as a clock', () => {
  expect(duration(0)).toBe('0:00');
  expect(duration(65.7)).toBe('1:05');
  expect(duration(3723)).toBe('1:02:03');
});

test('a playing queue moves on with the clock, a paused one does not', () => {
  const queue = { elapsed_time: 10, elapsed_time_last_updated: 1000, state: 'playing' };
  expect(elapsedNow(queue, 1_004_000)).toBe(14);
  expect(elapsedNow({ ...queue, state: 'paused' }, 1_004_000)).toBe(10);
});
