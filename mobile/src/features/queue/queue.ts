import type { PlayerQueue, QueueItem } from '@bopsie/client/api/schemas/player';

/** A queue as the queue sheet shows it. Items that have already played are left out. */
export interface QueueView {
  /**
   * The current item and any the player has been handed after it. The server
   * will not move or remove these, so they stay where they are.
   */
  held: QueueItem[];
  /** Everything after, in play order: what can be moved and removed. */
  upNext: QueueItem[];
  /** The queue index of the first item in `upNext`. */
  firstFree: number;
}

/** Splits a queue's items into those the player holds and those still to come. */
export function describeQueue(queue: PlayerQueue, items: QueueItem[]): QueueView {
  // The same bound the server checks edits against.
  const firstFree = Math.max(queue.current_index ?? -1, queue.index_in_buffer ?? -1) + 1;
  const start = Math.min(queue.current_index ?? firstFree, firstFree);
  return {
    held: items.slice(start, firstFree),
    upNext: items.slice(firstFree),
    firstFree,
  };
}

/** `items` with the one at index `from` moved to index `to`. */
export function moveItem<T>(items: readonly T[], from: number, to: number): T[] {
  const moved = items.slice();
  const [item] = moved.splice(from, 1);
  if (item !== undefined) moved.splice(to, 0, item);
  return moved;
}
