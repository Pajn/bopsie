import type { LogLine } from '@bopsie/client/api/schemas/admin';
import { append, KEPT, lineText, visible } from './model';

const line = (seq: number, level = 'INFO', message = 'hello', fields = {}): LogLine => ({
  seq,
  unix_ms: 0,
  level,
  target: 'bopsie_server',
  message,
  fields,
});

test('new lines are added once, and only the latest are kept', () => {
  const first = append([], [line(1), line(2)]);
  expect(append(first, [line(2), line(3)]).map((l) => l.seq)).toEqual([1, 2, 3]);
  const many = Array.from({ length: KEPT + 5 }, (_, i) => line(i + 1));
  const kept = append([], many);
  expect(kept).toHaveLength(KEPT);
  expect(kept[0]?.seq).toBe(6);
});

test('lines filter by level and text, fields included', () => {
  const lines = [
    line(1, 'DEBUG', 'telemetry'),
    line(2, 'INFO', 'volume set', { speaker: 'kitchen' }),
    line(3, 'WARN', 'scan failed'),
  ];
  expect(lineText(lines[1] as LogLine)).toBe('volume set speaker=kitchen');
  expect(visible(lines, 'INFO', '').map((l) => l.seq)).toEqual([2, 3]);
  expect(visible(lines, 'TRACE', 'KITCHEN').map((l) => l.seq)).toEqual([2]);
  expect(visible(lines, 'WARN', 'bopsie_server').map((l) => l.seq)).toEqual([3]);
});
