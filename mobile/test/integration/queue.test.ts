import { randomUUID } from 'node:crypto';
import type { MaConnection } from '@/api/client';
import {
  getQueue,
  getQueueItems,
  moveQueueItem,
  playMedia,
  playQueueItem,
  queueCommand,
  removeQueueItem,
} from '@/api/endpoints/players';
import type { QueueItem } from '@/api/schemas/player';
import { describeQueue } from '@/features/queue/queue';
import { registerPlayer, type TestPlayer } from './sendspinPlayer';
import { connect, describeServer } from './server';

async function eventually(check: () => Promise<boolean>, timeoutMs = 20_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!(await check())) {
    if (Date.now() > deadline) throw new Error('Condition did not hold in time');
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}

const ids = (items: QueueItem[]) => items.map((item) => item.queue_item_id);

/**
 * The queue sheet's edits against a Sendspin player holding both albums,
 * stopped so that what the player holds stays put while the queue changes.
 */
describeServer('queue', () => {
  let connection: MaConnection;
  let player: TestPlayer;
  const id = `bopsie-test-${randomUUID()}`;

  const view = async () => {
    const queue = await getQueue(connection, id);
    if (!queue) throw new Error('No queue');
    return describeQueue(queue, await getQueueItems(connection, id));
  };

  beforeAll(async () => {
    const session = await connect();
    connection = session.connection;
    player = await registerPlayer(session, id, 'Queue test');
    await playMedia(connection, id, ['library://album/1', 'library://album/2']);
    await eventually(async () => (await getQueue(connection, id))?.state === 'playing');
    await queueCommand(connection, id, 'stop');
    await eventually(async () => (await getQueue(connection, id))?.state === 'idle');
  });

  afterAll(async () => {
    await queueCommand(connection, id, 'stop').catch(() => {});
    player?.close();
    connection?.close();
  });

  it('lists every item and holds the current one', async () => {
    expect(await getQueueItems(connection, id)).toHaveLength(6);
    const { held, upNext } = await view();
    expect(held[0]?.name).toContain('Opening');
    expect(held.length + upNext.length).toBe(6);
    expect(upNext.length).toBeGreaterThanOrEqual(3);
  });

  it('moves the last item up to play next', async () => {
    const { upNext, firstFree } = await view();
    const last = upNext[upNext.length - 1] as QueueItem;
    await moveQueueItem(connection, id, last.queue_item_id, -(upNext.length - 1));
    await eventually(async () => (await view()).upNext[0]?.queue_item_id === last.queue_item_id);
    const after = await view();
    expect(after.firstFree).toBe(firstFree);
    expect(ids(after.upNext)).toEqual([last, ...upNext.slice(0, -1)].map((i) => i.queue_item_id));
  });

  it('refuses to move what the player holds', async () => {
    const { held } = await view();
    const current = held[0] as QueueItem;
    await expect(moveQueueItem(connection, id, current.queue_item_id, 1)).rejects.toThrow();
  });

  it('removes an item', async () => {
    const { upNext } = await view();
    const gone = upNext[1] as QueueItem;
    await removeQueueItem(connection, id, gone.queue_item_id);
    await eventually(async () => (await getQueueItems(connection, id)).length === 5);
    expect(ids((await view()).upNext)).not.toContain(gone.queue_item_id);
  });

  it('plays from an item further down', async () => {
    const { upNext } = await view();
    const target = upNext[upNext.length - 1] as QueueItem;
    await playQueueItem(connection, id, target.queue_item_id);
    await eventually(
      async () =>
        (await getQueue(connection, id))?.current_item?.queue_item_id === target.queue_item_id,
    );
  });
});
