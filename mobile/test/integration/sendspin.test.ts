import { randomUUID } from 'node:crypto';
import type { MaConnection } from '@bopsie/client/api/client';
import { getPlayer, getQueue, playMedia, queueCommand } from '@bopsie/client/api/endpoints/players';
import { registerPlayer, type TestPlayer } from './sendspinPlayer';
import { albumUri, connect, describeServer } from './server';

describeServer('Sendspin player', () => {
  let connection: MaConnection;
  let player: TestPlayer;
  const clientId = `bopsie-test-${randomUUID()}`;

  beforeAll(async () => {
    const session = await connect();
    connection = session.connection;
    player = await registerPlayer(session, clientId, 'Integration test phone');
  });

  afterAll(() => {
    player?.close();
    connection?.close();
  });

  it('registers as a standalone player with its own queue', async () => {
    const registered = await getPlayer(connection, clientId);
    expect(registered).toMatchObject({ player_id: clientId, type: 'player', available: true });
    expect(await getQueue(connection, clientId)).toMatchObject({ queue_id: clientId });
  });

  it('streams PCM with big-endian server timestamps when the API plays to it', async () => {
    await playMedia(connection, clientId, await albumUri(connection, 'First Album'));
    const start = await player.message('stream/start');
    expect(start.payload?.player).toMatchObject({ codec: 'pcm', channels: 2, bit_depth: 16 });

    const chunk = await player.chunk();
    expect(chunk[0]).toBe(4);
    const view = new DataView(chunk.buffer, chunk.byteOffset, chunk.byteLength);
    const timestamp = view.getBigInt64(1, false);
    const serverTime = (await player.message('server/time')).payload?.server_transmitted as number;
    // Scheduled ahead of the server clock, not decades away as a misread byte order would give.
    expect(Number(timestamp)).toBeGreaterThan(serverTime);
    expect(Number(timestamp) - serverTime).toBeLessThan(10 * 60 * 1_000_000);
    expect((chunk.length - 9) % 4).toBe(0);

    // The server sends only changed fields, so the title can come in any state update.
    const state = await player.message('server/state', (payload) => {
      const metadata = payload.metadata as { title?: string } | undefined;
      return typeof metadata?.title === 'string';
    });
    expect(state.payload?.metadata).toMatchObject({ title: 'Opening', album: 'First Album' });
  });

  it('ends the stream when the API pauses the queue', async () => {
    await queueCommand(connection, clientId, 'pause');
    await player.message('stream/end');
  });
});
