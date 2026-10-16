import { playerQueue } from '@bopsie/client/api/schemas/player';
import { formatDuration, queuePosition } from './progress';

const queue = (fields: object) =>
  playerQueue.parse({
    queue_id: 'q',
    elapsed_time: 30,
    elapsed_time_last_updated: 1000,
    current_item: { queue_id: 'q', queue_item_id: 'i', name: 'Song', duration: 200 },
    ...fields,
  });

describe('queuePosition', () => {
  it('moves on from the reported time while playing', () => {
    expect(queuePosition(queue({ state: 'playing' }), 1_012_500)).toBe(42.5);
  });

  it('stays put while it buffers, though it is meant to be playing', () => {
    expect(queuePosition(queue({ state: 'playing', buffering: true }), 1_012_500)).toBe(30);
  });

  it('stays put while paused', () => {
    expect(queuePosition(queue({ state: 'paused' }), 1_012_500)).toBe(30);
  });

  it('stops at the end of the item', () => {
    expect(queuePosition(queue({ state: 'playing' }), 2_000_000)).toBe(200);
  });
});

test.each([
  [0, '0:00'],
  [65.9, '1:05'],
  [3725, '1:02:05'],
])('formatDuration(%s) is %s', (seconds, text) => {
  expect(formatDuration(seconds)).toBe(text);
});
