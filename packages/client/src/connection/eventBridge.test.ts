import { QueryClient } from '@tanstack/react-query';
import type { Player, PlayerQueue } from '../api/schemas/player';
import { applyEvent, LIBRARY_REFRESH_MS } from './eventBridge';
import { queryKeys } from './queryKeys';

const QUEUE = {
  queue_id: 'phone',
  state: 'playing',
  elapsed_time: 3,
  elapsed_time_last_updated: 100,
  items: 2,
};

const PLAYER = { provider: 'sendspin', available: true, playback_state: 'idle' };

const clients: QueryClient[] = [];

function setup() {
  // No garbage-collection timers, which would keep Jest running.
  const client = new QueryClient({
    defaultOptions: { queries: { gcTime: Number.POSITIVE_INFINITY } },
  });
  clients.push(client);
  const key = queryKeys.queue('acct', 'phone');
  // Only queues the app shows are kept.
  client.setQueryData(key, null);
  return { client, key };
}

afterEach(() => {
  for (const client of clients.splice(0)) client.clear();
});

describe('applyEvent', () => {
  it('replaces the cached queue with an updated one', () => {
    const { client, key } = setup();
    applyEvent(client, 'acct', {
      event: 'queue_updated',
      object_id: 'phone',
      data: QUEUE,
    });
    expect(client.getQueryData<PlayerQueue>(key)?.state).toBe('playing');
  });

  it('ignores queues the app does not show', () => {
    const { client } = setup();
    applyEvent(client, 'acct', {
      event: 'queue_updated',
      object_id: 'kitchen',
      data: { ...QUEUE, queue_id: 'kitchen' },
    });
    expect(client.getQueryState(queryKeys.queue('acct', 'kitchen'))).toBeUndefined();
  });

  it('moves only the position on time updates', () => {
    const { client, key } = setup();
    applyEvent(client, 'acct', {
      event: 'queue_updated',
      object_id: 'phone',
      data: QUEUE,
    });
    applyEvent(
      client,
      'acct',
      { event: 'queue_time_updated', object_id: 'phone', data: 42 },
      500_000,
    );
    expect(client.getQueryData<PlayerQueue>(key)).toMatchObject({
      elapsed_time: 42,
      elapsed_time_last_updated: 500,
      items: 2,
    });
  });

  it('marks the cached items stale when they change', () => {
    const { client } = setup();
    const items = queryKeys.queueItems('acct', 'phone');
    client.setQueryData(items, []);
    applyEvent(client, 'acct', { event: 'queue_items_updated', object_id: 'phone', data: QUEUE });
    expect(client.getQueryData<PlayerQueue>(queryKeys.queue('acct', 'phone'))?.items).toBe(2);
    expect(client.getQueryState(items)?.isInvalidated).toBe(true);
  });

  it('leaves the cached items alone on other queue updates', () => {
    const { client } = setup();
    const items = queryKeys.queueItems('acct', 'phone');
    client.setQueryData(items, []);
    applyEvent(client, 'acct', { event: 'queue_updated', object_id: 'phone', data: QUEUE });
    expect(client.getQueryState(items)?.isInvalidated).toBe(false);
  });

  it('replaces an updated player in the cached list', () => {
    const { client } = setup();
    const key = queryKeys.players('acct');
    client.setQueryData(key, [
      { ...PLAYER, player_id: 'kitchen', name: 'Kitchen' },
      { ...PLAYER, player_id: 'living', name: 'Living room' },
    ]);
    applyEvent(client, 'acct', {
      event: 'player_updated',
      object_id: 'living',
      data: { ...PLAYER, player_id: 'living', name: 'Living room', synced_to: 'kitchen' },
    });
    const players = client.getQueryData<Player[]>(key);
    expect(players?.map((p) => [p.player_id, p.synced_to ?? null])).toEqual([
      ['kitchen', null],
      ['living', 'kitchen'],
    ]);
  });

  it('drops a removed player from the cached list', () => {
    const { client } = setup();
    const key = queryKeys.players('acct');
    client.setQueryData(key, [{ ...PLAYER, player_id: 'kitchen', name: 'Kitchen' }]);
    applyEvent(client, 'acct', { event: 'player_removed', object_id: 'kitchen', data: null });
    expect(client.getQueryData(key)).toEqual([]);
  });

  it('leaves the player list to its first fetch', () => {
    const { client } = setup();
    applyEvent(client, 'acct', {
      event: 'player_added',
      object_id: 'kitchen',
      data: { ...PLAYER, player_id: 'kitchen', name: 'Kitchen' },
    });
    expect(client.getQueryData(queryKeys.players('acct'))).toBeUndefined();
  });

  it('marks library listings stale when the library changes', async () => {
    const { client } = setup();
    const key = queryKeys.libraryList('acct', 'albums', '');
    client.setQueryData(key, []);
    applyEvent(client, 'acct', { event: 'media_item_added', object_id: 'x', data: {} });
    expect(client.getQueryState(key)?.isInvalidated).toBe(true);
  });

  it('refreshes the library once for a burst of changes, and again after it', () => {
    jest.useFakeTimers();
    try {
      const { client } = setup();
      const invalidate = jest.spyOn(client, 'invalidateQueries');
      const change = { event: 'media_item_updated', object_id: 'x', data: {} };
      for (let i = 0; i < 500; i++) applyEvent(client, 'acct', change);
      expect(invalidate).toHaveBeenCalledTimes(1);
      jest.advanceTimersByTime(LIBRARY_REFRESH_MS);
      expect(invalidate).toHaveBeenCalledTimes(2);
      jest.advanceTimersByTime(LIBRARY_REFRESH_MS);
      expect(invalidate).toHaveBeenCalledTimes(2);
      // A change on its own after that shows at once.
      applyEvent(client, 'acct', change);
      expect(invalidate).toHaveBeenCalledTimes(3);
    } finally {
      jest.useRealTimers();
    }
  });
});
