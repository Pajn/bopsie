import { randomUUID } from 'node:crypto';
import type { MaConnection } from '@/api/client';
import { getPlayer, getQueue, playMedia, queueCommand } from '@/api/endpoints/players';
import { socketUrl } from '@/api/url';
import { connect, describeServer } from './server';

/**
 * The server's side of what the Android player relies on, spoken from Node
 * with the same messages as `SendspinProtocol.kt`: a token-authenticated
 * `/sendspin` connection registers a player that the API can play to, and
 * audio arrives as type-4 frames with big-endian server timestamps.
 */
class TestPlayer {
  private socket: WebSocket;
  private messages: Array<{ type: string; payload?: Record<string, unknown> }> = [];
  private chunks: Uint8Array[] = [];
  private waiters: Array<() => void> = [];

  constructor(url: string) {
    this.socket = new WebSocket(url);
    this.socket.binaryType = 'arraybuffer';
    this.socket.onmessage = (event) => {
      if (typeof event.data === 'string') this.messages.push(JSON.parse(event.data));
      else this.chunks.push(new Uint8Array(event.data as ArrayBuffer));
      for (const wake of this.waiters.splice(0)) wake();
    };
  }

  opened() {
    return new Promise<void>((resolve, reject) => {
      this.socket.onopen = () => resolve();
      this.socket.onerror = () => reject(new Error('Could not open the Sendspin socket'));
    });
  }

  send(message: object) {
    this.socket.send(JSON.stringify(message));
  }

  /** Waits until a message of `type` that matches `where` has arrived, then returns the first one. */
  async message(
    type: string,
    where: (payload: Record<string, unknown>) => boolean = () => true,
    timeoutMs = 15_000,
  ) {
    return this.until(
      () => this.messages.find((m) => m.type === type && where(m.payload ?? {})),
      timeoutMs,
      type,
    );
  }

  async chunk(timeoutMs = 15_000) {
    return this.until(() => this.chunks[0], timeoutMs, 'an audio chunk');
  }

  private async until<T>(find: () => T | undefined, timeoutMs: number, what: string): Promise<T> {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      const found = find();
      if (found !== undefined) return found;
      if (Date.now() > deadline) throw new Error(`Timed out waiting for ${what}`);
      await new Promise<void>((resolve) => {
        this.waiters.push(resolve);
        setTimeout(resolve, 200);
      });
    }
  }

  close() {
    this.send({ type: 'client/goodbye', payload: { reason: 'user_request' } });
    this.socket.close();
  }
}

describeServer('Sendspin player', () => {
  let connection: MaConnection;
  let player: TestPlayer;
  const clientId = `bopsie-test-${randomUUID()}`;

  beforeAll(async () => {
    const session = await connect();
    connection = session.connection;
    player = new TestPlayer(socketUrl(session.account.serverUrl, '/sendspin'));
    await player.opened();
    player.send({ type: 'auth', token: session.token, client_id: clientId });
    await player.message('auth_ok');

    const format = { codec: 'pcm', channels: 2, sample_rate: 48000, bit_depth: 16 };
    const support = {
      supported_formats: [format],
      buffer_capacity: 1_048_576,
      supported_commands: ['volume', 'mute'],
    };
    player.send({
      type: 'client/hello',
      payload: {
        client_id: clientId,
        name: 'Integration test phone',
        version: 1,
        supported_roles: ['player@v1', 'controller@v1', 'metadata@v1'],
        device_info: {
          product_name: 'Mobile Application',
          manufacturer: 'Bopsie',
          software_version: '0.0.0',
        },
        'player@v1_support': support,
        player_support: support,
      },
    });
    await player.message('server/hello');
    player.send({ type: 'client/time', payload: { client_transmitted: 1 } });
    await player.message('server/time');
    player.send({
      type: 'client/state',
      payload: {
        available: true,
        state: 'synchronized',
        player: {
          volume: 50,
          muted: false,
          static_delay_ms: 0,
          required_lead_time_ms: 300,
          min_buffer_ms: 300,
        },
      },
    });

    // Registration completes asynchronously after the first state.
    for (let i = 0; i < 40; i++) {
      if ((await getPlayer(connection, clientId))?.available) break;
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
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
    await playMedia(connection, clientId, 'library://album/1');
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
