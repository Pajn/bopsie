import type { QueryClient } from '@tanstack/react-query';
import type { MaEvent } from '@/api/client';
import { type PlayerQueue, playerQueue } from '@/api/schemas/player';
import { queryKeys } from './queryKeys';

/** Library changes that make cached listings stale. */
const LIBRARY_EVENTS = new Set([
  'media_item_added',
  'media_item_updated',
  'media_item_deleted',
  'music_sync_completed',
]);

/**
 * Applies a server event to the query cache. Queue updates carry the whole
 * queue and replace the cached one; time updates move only the position, and
 * arrive often enough that refetching on each would be wasteful.
 */
export function applyEvent(
  queryClient: QueryClient,
  accountId: string,
  queueId: string | null,
  event: MaEvent,
  nowMs: number = Date.now(),
): void {
  if (LIBRARY_EVENTS.has(event.event)) {
    void queryClient.invalidateQueries({ queryKey: queryKeys.library(accountId) });
    return;
  }
  if (!queueId || event.object_id !== queueId) return;
  const key = queryKeys.queue(accountId, queueId);

  switch (event.event) {
    case 'queue_added':
    case 'queue_updated':
    case 'queue_items_updated': {
      const parsed = playerQueue.safeParse(event.data);
      if (parsed.success) queryClient.setQueryData(key, parsed.data);
      else void queryClient.invalidateQueries({ queryKey: key });
      return;
    }
    case 'queue_time_updated': {
      if (typeof event.data !== 'number') return;
      const elapsed = event.data;
      queryClient.setQueryData<PlayerQueue | null>(key, (queue) =>
        queue
          ? { ...queue, elapsed_time: elapsed, elapsed_time_last_updated: nowMs / 1000 }
          : queue,
      );
      return;
    }
  }
}
