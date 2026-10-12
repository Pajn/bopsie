import { QueryClient } from '@tanstack/react-query';
import type { PlayerQueue } from '@/api/schemas/player';
import { applyEvent } from './eventBridge';
import { queryKeys } from './queryKeys';

const QUEUE = {
  queue_id: 'phone',
  state: 'playing',
  elapsed_time: 3,
  elapsed_time_last_updated: 100,
  items: 2,
};

const clients: QueryClient[] = [];

function setup() {
  // No garbage-collection timers, which would keep Jest running.
  const client = new QueryClient({
    defaultOptions: { queries: { gcTime: Number.POSITIVE_INFINITY } },
  });
  clients.push(client);
  return { client, key: queryKeys.queue('acct', 'phone') };
}

afterEach(() => {
  for (const client of clients.splice(0)) client.clear();
});

describe('applyEvent', () => {
  it('replaces the cached queue with an updated one', () => {
    const { client, key } = setup();
    applyEvent(client, 'acct', 'phone', {
      event: 'queue_updated',
      object_id: 'phone',
      data: QUEUE,
    });
    expect(client.getQueryData<PlayerQueue>(key)?.state).toBe('playing');
  });

  it('ignores other players’ queues', () => {
    const { client, key } = setup();
    applyEvent(client, 'acct', 'phone', {
      event: 'queue_updated',
      object_id: 'kitchen',
      data: QUEUE,
    });
    expect(client.getQueryData(key)).toBeUndefined();
  });

  it('moves only the position on time updates', () => {
    const { client, key } = setup();
    applyEvent(client, 'acct', 'phone', {
      event: 'queue_updated',
      object_id: 'phone',
      data: QUEUE,
    });
    applyEvent(
      client,
      'acct',
      'phone',
      { event: 'queue_time_updated', object_id: 'phone', data: 42 },
      500_000,
    );
    expect(client.getQueryData<PlayerQueue>(key)).toMatchObject({
      elapsed_time: 42,
      elapsed_time_last_updated: 500,
      items: 2,
    });
  });

  it('marks library listings stale when the library changes', async () => {
    const { client } = setup();
    const key = queryKeys.libraryList('acct', 'albums', '');
    client.setQueryData(key, []);
    applyEvent(client, 'acct', 'phone', { event: 'media_item_added', object_id: 'x', data: {} });
    expect(client.getQueryState(key)?.isInvalidated).toBe(true);
  });
});
