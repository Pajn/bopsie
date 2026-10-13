import { type PlayerQueue, playerQueue, type QueueItem, queueItem } from '@/api/schemas/player';
import { describeQueue, moveItem } from './queue';

const ITEMS: QueueItem[] = ['a', 'b', 'c', 'd', 'e'].map((id) =>
  queueItem.parse({ queue_id: 'phone', queue_item_id: id, name: id }),
);

const queue = (fields: Record<string, unknown>): PlayerQueue =>
  playerQueue.parse({ queue_id: 'phone', items: ITEMS.length, ...fields });

const ids = (items: QueueItem[]) => items.map((item) => item.queue_item_id);

describe('describeQueue', () => {
  it('holds the current item and leaves the rest free', () => {
    const view = describeQueue(queue({ current_index: 1, index_in_buffer: 1 }), ITEMS);
    expect(ids(view.held)).toEqual(['b']);
    expect(ids(view.upNext)).toEqual(['c', 'd', 'e']);
    expect(view.firstFree).toBe(2);
  });

  it('holds the next item too once the player has been handed it', () => {
    const view = describeQueue(queue({ current_index: 1, index_in_buffer: 2 }), ITEMS);
    expect(ids(view.held)).toEqual(['b', 'c']);
    expect(ids(view.upNext)).toEqual(['d', 'e']);
    expect(view.firstFree).toBe(3);
  });

  it('holds only the current item when repeat has wrapped the buffer to the front', () => {
    const view = describeQueue(queue({ current_index: 4, index_in_buffer: 0 }), ITEMS);
    expect(ids(view.held)).toEqual(['e']);
    expect(ids(view.upNext)).toEqual([]);
  });

  it('leaves everything free before playback starts', () => {
    const view = describeQueue(queue({}), ITEMS);
    expect(view.held).toEqual([]);
    expect(ids(view.upNext)).toEqual(['a', 'b', 'c', 'd', 'e']);
    expect(view.firstFree).toBe(0);
  });

  it('holds what the player has been handed before the queue names a current item', () => {
    const view = describeQueue(queue({ index_in_buffer: 0 }), ITEMS);
    expect(ids(view.held)).toEqual([]);
    expect(ids(view.upNext)).toEqual(['b', 'c', 'd', 'e']);
  });
});

describe('moveItem', () => {
  it('moves an item later', () => {
    expect(moveItem(['a', 'b', 'c', 'd'], 1, 3)).toEqual(['a', 'c', 'd', 'b']);
  });

  it('moves an item earlier', () => {
    expect(moveItem(['a', 'b', 'c', 'd'], 3, 1)).toEqual(['a', 'd', 'b', 'c']);
  });

  it('leaves the original alone', () => {
    const items = ['a', 'b'];
    moveItem(items, 0, 1);
    expect(items).toEqual(['a', 'b']);
  });
});
