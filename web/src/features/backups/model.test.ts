import { formatBytes, scheduleLabel, toLocal, toUtc } from './model';

// Stockholm in summer: UTC is two hours behind, so the offset is -120.
const STOCKHOLM = -120;

test('a local time of the week is UTC and back, crossing midnight', () => {
  expect(toUtc({ hour: 3, weekday: 0 }, STOCKHOLM)).toEqual({ hour: 1, weekday: 0 });
  expect(toUtc({ hour: 1, weekday: 0 }, STOCKHOLM)).toEqual({ hour: 23, weekday: 6 });
  expect(toLocal({ hour: 23, weekday: 6 }, STOCKHOLM)).toEqual({ hour: 1, weekday: 0 });
  // New York: UTC is four hours ahead.
  expect(toUtc({ hour: 22, weekday: 6 }, 240)).toEqual({ hour: 2, weekday: 0 });
});

test('sizes read in their unit', () => {
  expect(formatBytes(512)).toBe('512 B');
  expect(formatBytes(3_400_000)).toBe('3.4 MB');
  expect(formatBytes(352_000_000)).toBe('352 MB');
  expect(formatBytes(1_200_000_000)).toBe('1.2 GB');
});

test('a schedule says when, in local time', () => {
  expect(scheduleLabel(null, 0)).toBe('Off');
  expect(scheduleLabel({ every: 'daily', hour: 1, weekday: 0, keep: 7 }, STOCKHOLM)).toBe(
    'Daily at 03:00, keeping 7',
  );
  expect(scheduleLabel({ every: 'weekly', hour: 23, weekday: 6, keep: 4 }, STOCKHOLM)).toBe(
    'Mondays at 01:00, keeping 4',
  );
});
