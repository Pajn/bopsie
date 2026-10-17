import type { QueryClient } from '@tanstack/react-query';
import type { MaEvent } from '../api/client';
import { type Player, type PlayerQueue, player, playerQueue } from '../api/schemas/player';
import { queryKeys } from './queryKeys';

/** Library changes that make cached listings stale. */
const LIBRARY_EVENTS = new Set([
  'media_item_added',
  'media_item_updated',
  'media_item_deleted',
  'music_sync_completed',
]);

/** How often a burst of library events refreshes what is shown, at most. */
export const LIBRARY_REFRESH_MS = 1000;

/** Per client and account: whether a refresh waits, and whether events came meanwhile. */
const refreshing = new WeakMap<QueryClient, Map<string, { more: boolean }>>();

/**
 * Marks the library's listings stale: at once for the first change, then at most once every
 * {@link LIBRARY_REFRESH_MS} while more come, and once after the last. A sync that changes
 * thousands of items sends an event for each, and refetching every listing on each would keep
 * the server busy reading the same pages over and over.
 */
function libraryChanged(queryClient: QueryClient, accountId: string): void {
  let accounts = refreshing.get(queryClient);
  if (!accounts) {
    accounts = new Map();
    refreshing.set(queryClient, accounts);
  }
  const waiting = accounts.get(accountId);
  if (waiting) {
    waiting.more = true;
    return;
  }
  void queryClient.invalidateQueries({ queryKey: queryKeys.library(accountId) });
  const state = { more: false };
  accounts.set(accountId, state);
  const timer = setTimeout(() => {
    accounts.delete(accountId);
    if (state.more) libraryChanged(queryClient, accountId);
  }, LIBRARY_REFRESH_MS);
  // In Node, it would keep a process alive that has nothing else to do.
  (timer as { unref?: () => void }).unref?.();
}

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
    libraryChanged(queryClient, accountId);
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
