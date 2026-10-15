import * as z from 'zod/mini';
import { MaConnection, reconnectDelayMs, type SocketLike } from './client';
import { CommandError, NetworkError, SchemaError, SetupRequiredError } from './errors';

const SERVER_INFO = {
  server_id: 'srv',
  server_version: '2.10.6',
  schema_version: 65,
  min_supported_schema_version: 28,
};

class FakeSocket implements SocketLike {
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onclose: ((event: { code: number; reason: string }) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  sent: Array<{ message_id: string; command: string; args: Record<string, unknown> }> = [];
  closed = false;

  constructor(readonly url: string) {}

  send(data: string) {
    this.sent.push(JSON.parse(data));
  }
  close() {
    this.closed = true;
  }
  receive(message: unknown) {
    this.onmessage?.({ data: JSON.stringify(message) });
  }
  drop(code = 1006) {
    this.onclose?.({ code, reason: '' });
  }
  last() {
    const message = this.sent.at(-1);
    if (!message) throw new Error('Nothing sent');
    return message;
  }
}

function setup(options: { token?: string; maxReconnectAttempts?: number } = {}) {
  const sockets: FakeSocket[] = [];
  const connection = new MaConnection({
    serverUrl: 'http://music.local:8095',
    createSocket: (url) => {
      const socket = new FakeSocket(url);
      sockets.push(socket);
      return socket;
    },
    ...options,
  });
  const socket = () => {
    const current = sockets.at(-1);
    if (!current) throw new Error('No socket opened');
    return current;
  };
  return { connection, sockets, socket };
}

/** Lets pending promise callbacks run. */
const flush = () => new Promise((resolve) => jest.requireActual('timers').setImmediate(resolve));

describe('MaConnection', () => {
  it('authenticates with the token after server info, then sends commands', async () => {
    const { connection, socket } = setup({ token: 'secret' });
    connection.connect();
    expect(socket().url).toBe('ws://music.local:8095/ws');

    const result = connection.command('players/all');
    socket().receive(SERVER_INFO);
    expect(socket().last()).toMatchObject({ command: 'auth', args: { token: 'secret' } });
    expect(connection.serverInfo?.server_version).toBe('2.10.6');

    socket().receive({ message_id: 'auth', result: { authenticated: true } });
    await flush();
    expect(connection.status).toBe('ready');
    const sent = socket().last();
    expect(sent.command).toBe('players/all');
    socket().receive({ message_id: sent.message_id, result: [{ player_id: 'p' }] });
    await expect(result).resolves.toEqual([{ player_id: 'p' }]);
  });

  it('is ready on server info alone without a token', async () => {
    const { connection, socket } = setup();
    const ready = connection.ready();
    socket().receive(SERVER_INFO);
    await expect(ready).resolves.toBeUndefined();
  });

  it('concatenates partial batches until the final message', async () => {
    const { connection, socket } = setup();
    connection.connect();
    socket().receive(SERVER_INFO);
    const result = connection.command('music/playlists/playlist_tracks');
    await flush();
    const { message_id } = socket().last();
    socket().receive({ message_id, result: [1, 2], partial: true });
    socket().receive({ message_id, result: [3], partial: true });
    socket().receive({ message_id, result: [4], partial: false });
    await expect(result).resolves.toEqual([1, 2, 3, 4]);
  });

  it('rejects with the server error and validates results against a schema', async () => {
    const { connection, socket } = setup();
    connection.connect();
    socket().receive(SERVER_INFO);

    const failing = connection.command('nope');
    await flush();
    socket().receive({
      message_id: socket().last().message_id,
      error_code: 12,
      details: 'Invalid',
    });
    await expect(failing).rejects.toEqual(new CommandError('nope', 12, 'Invalid'));

    const mismatched = connection.command('count', {}, { schema: z.number() });
    await flush();
    socket().receive({ message_id: socket().last().message_id, result: 'many' });
    await expect(mismatched).rejects.toBeInstanceOf(SchemaError);
  });

  it('delivers events to listeners', () => {
    const { connection, socket } = setup();
    const events: unknown[] = [];
    connection.onEvent((event) => events.push(event));
    connection.connect();
    socket().receive({ event: 'queue_time_updated', object_id: 'q', data: 12.5 });
    expect(events).toEqual([{ event: 'queue_time_updated', object_id: 'q', data: 12.5 }]);
  });

  it('stops for good when the token is rejected', async () => {
    const { connection, socket, sockets } = setup({ token: 'old' });
    const ready = connection.ready();
    socket().receive(SERVER_INFO);
    socket().receive({ message_id: 'auth', error_code: 23, details: 'Invalid token' });
    await expect(ready).rejects.toMatchObject({ isUnauthorized: true });
    expect(connection.status).toBe('unauthorized');
    connection.connect();
    expect(sockets).toHaveLength(1);
  });

  it('reports a server that still needs setup', async () => {
    const { connection, socket } = setup({ token: 't' });
    const ready = connection.ready();
    socket().receive(SERVER_INFO);
    socket().receive({ message_id: 'connection', error_code: 503, details: 'Setup required' });
    await expect(ready).rejects.toBeInstanceOf(SetupRequiredError);
    expect(connection.status).toBe('closed');
  });

  describe('reconnection', () => {
    beforeEach(() => jest.useFakeTimers());
    afterEach(() => jest.useRealTimers());

    it('fails commands in flight, then reconnects with backoff and serves waiting commands', async () => {
      const { connection, socket, sockets } = setup({ token: 't' });
      connection.connect();
      socket().receive(SERVER_INFO);
      socket().receive({ message_id: 'auth', result: {} });

      const inFlight = connection.command('slow');
      await Promise.resolve();
      await Promise.resolve();
      socket().drop();
      await expect(inFlight).rejects.toBeInstanceOf(NetworkError);
      expect(connection.status).toBe('reconnecting');

      const later = connection.command('players/all');
      jest.advanceTimersByTime(reconnectDelayMs(0));
      expect(sockets).toHaveLength(2);
      socket().receive(SERVER_INFO);
      socket().receive({ message_id: 'auth', result: {} });
      await Promise.resolve();
      await Promise.resolve();
      socket().receive({ message_id: socket().last().message_id, result: [] });
      await expect(later).resolves.toEqual([]);
    });

    it('gives up after the configured number of attempts', () => {
      const { connection, socket, sockets } = setup({ maxReconnectAttempts: 2 });
      connection.connect();
      socket().receive(SERVER_INFO);
      socket().drop();
      jest.advanceTimersByTime(reconnectDelayMs(0));
      socket().drop();
      jest.advanceTimersByTime(reconnectDelayMs(1));
      socket().drop();
      expect(sockets).toHaveLength(3);
      expect(connection.status).toBe('closed');
    });

    it('connects again on a retry after giving up, with its retries back', () => {
      const { connection, socket, sockets } = setup({ maxReconnectAttempts: 1 });
      connection.connect();
      socket().receive(SERVER_INFO);
      socket().drop();
      jest.advanceTimersByTime(reconnectDelayMs(0));
      socket().drop();
      expect(connection.status).toBe('closed');

      connection.retry();
      expect(sockets).toHaveLength(3);
      expect(connection.status).toBe('connecting');
      socket().drop();
      expect(connection.status).toBe('reconnecting');
    });

    it('connects at once on a retry while waiting out a backoff', () => {
      const { connection, socket, sockets } = setup();
      connection.connect();
      socket().receive(SERVER_INFO);
      socket().drop();
      expect(connection.status).toBe('reconnecting');
      connection.retry();
      expect(sockets).toHaveLength(2);
      jest.advanceTimersByTime(60_000);
      expect(sockets).toHaveLength(2);
    });

    it('ignores a retry while connected', () => {
      const { connection, sockets } = setup();
      connection.connect();
      connection.retry();
      expect(sockets).toHaveLength(1);
    });

    it('does not reconnect after an explicit close', () => {
      const { connection, socket, sockets } = setup();
      connection.connect();
      socket().receive(SERVER_INFO);
      connection.close();
      jest.advanceTimersByTime(60_000);
      expect(sockets).toHaveLength(1);
      expect(socket().closed).toBe(true);
    });
  });
});

test('reconnect delays double up to thirty seconds', () => {
  expect([0, 1, 2, 3, 4, 5, 9].map(reconnectDelayMs)).toEqual([
    1000, 2000, 4000, 8000, 16000, 30000, 30000,
  ]);
});
