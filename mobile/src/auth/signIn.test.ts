import type { SocketLike } from '@/api/client';
import { SignInError, signIn } from './signIn';

type Handler = (command: string, args: Record<string, unknown>, token: string | null) => unknown;

/** A fake server: answers each command through `handler`, after server info and auth. */
function fakeServer(handler: Handler, info: object = { schema_version: 65 }) {
  const commands: Array<{ command: string; token: string | null }> = [];
  const createSocket = (): SocketLike => {
    let token: string | null = null;
    const socket: SocketLike = {
      onopen: null,
      onmessage: null,
      onclose: null,
      onerror: null,
      send(data) {
        const { message_id, command, args } = JSON.parse(data);
        const reply = (message: object) =>
          queueMicrotask(() =>
            socket.onmessage?.({ data: JSON.stringify({ message_id, ...message }) }),
          );
        if (command === 'auth') {
          token = args.token;
          reply({ result: { authenticated: true } });
          return;
        }
        commands.push({ command, token });
        reply({ result: handler(command, args, token) ?? null });
      },
      close() {},
    };
    queueMicrotask(() =>
      socket.onmessage?.({
        data: JSON.stringify({ server_id: 'srv', server_version: '2.10.6', ...info }),
      }),
    );
    return socket;
  };
  return { createSocket, commands };
}

const input = {
  serverUrl: 'music.local',
  username: 'alice',
  password: 'hunter22',
  deviceName: 'Bopsie on Android',
};

describe('signIn', () => {
  it('logs in, mints a long-lived token, and revokes the session', async () => {
    const server = fakeServer((command, args) => {
      if (command === 'auth/login') {
        expect(args).toMatchObject({ username: 'alice', password: 'hunter22' });
        return {
          success: true,
          access_token: 'session',
          user: { user_id: 'u1', username: 'alice', role: 'admin' },
        };
      }
      if (command === 'auth/token/create') return 'long-lived';
    });

    const result = await signIn(input, { createSocket: server.createSocket, now: () => 5 });

    expect(result).toEqual({
      token: 'long-lived',
      account: {
        id: 'srv#u1',
        serverUrl: 'http://music.local:8095',
        serverId: 'srv',
        userId: 'u1',
        username: 'alice',
        addedAt: 5,
      },
    });
    expect(server.commands).toEqual([
      { command: 'auth/login', token: null },
      { command: 'auth/token/create', token: 'session' },
      { command: 'auth/logout', token: 'session' },
    ]);
  });

  it('reports wrong credentials from the login result', async () => {
    const server = fakeServer(() => ({ success: false, error: 'Invalid username or password' }));
    await expect(signIn(input, { createSocket: server.createSocket })).rejects.toEqual(
      new SignInError('Invalid username or password'),
    );
  });

  it('refuses servers older than 2.7', async () => {
    const server = fakeServer(() => null, { server_version: '2.6.3', schema_version: 27 });
    await expect(signIn(input, { createSocket: server.createSocket })).rejects.toThrow(
      'Bopsie needs 2.7 or later',
    );
    expect(server.commands).toEqual([]);
  });
});
