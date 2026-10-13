import type { QueryClient } from '@tanstack/react-query';
import type { MaEvent } from '@/api/client';
import { type Player, type PlayerQueue, player, playerQueue } from '@/api/schemas/player';
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
 * arrive often enough that refetching on each would be wasteful. Only queues
 * already cached are kept, so the cache holds the queues the app shows rather
 * than every queue on the server. A change to a queue's items carries the
 * queue but not the items, so they are fetched again. Player updates carry the whole player and
 * replace it in the cached list.
 */
export function applyEvent(
  queryClient: QueryClient,
  accountId: string,
  event: MaEvent,
  nowMs: number = Date.now(),
): void {
  if (LIBRARY_EVENTS.has(event.event)) {
    void queryClient.invalidateQueries({ queryKey: queryKeys.library(accountId) });
    return;
  }
  if (!event.object_id) return;

  switch (event.event) {
    case 'player_added':
    case 'player_updated': {
      const key = queryKeys.players(accountId);
      if (!queryClient.getQueryData(key)) return;
      const parsed = player.safeParse(event.data);
      if (!parsed.success) {
        void queryClient.invalidateQueries({ queryKey: key });
        return;
      }
      const updated = parsed.data;
      queryClient.setQueryData<Player[]>(key, (players = []) => [
        ...players.filter((p) => p.player_id !== updated.player_id),
        updated,
      ]);
      return;
    }
    case 'player_removed': {
      const id = event.object_id;
      queryClient.setQueryData<Player[]>(queryKeys.players(accountId), (players) =>
        players?.filter((p) => p.player_id !== id),
      );
      return;
    }
  }

  const key = queryKeys.queue(accountId, event.object_id);
  if (queryClient.getQueryState(key) === undefined) return;

  switch (event.event) {
    case 'queue_added':
    case 'queue_updated':
    case 'queue_items_updated': {
      const parsed = playerQueue.safeParse(event.data);
      if (parsed.success) queryClient.setQueryData(key, parsed.data);
      else void queryClient.invalidateQueries({ queryKey: key });
      if (event.event === 'queue_items_updated') {
        void queryClient.invalidateQueries({
          queryKey: queryKeys.queueItems(accountId, event.object_id),
        });
      }
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
