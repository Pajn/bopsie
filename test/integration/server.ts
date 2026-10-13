import { MaConnection, type SocketLike } from '@/api/client';
import { signIn } from '@/auth/signIn';

export const MA_URL = process.env.MA_URL;
export const MA_USERNAME = process.env.MA_USERNAME ?? 'bopsie';
export const MA_PASSWORD = process.env.MA_PASSWORD ?? 'bopsie-test';

/** `describe` when a server is configured, `describe.skip` otherwise. */
export const describeServer = MA_URL ? describe : describe.skip;

/** Node's built-in WebSocket, which the app's client speaks to like React Native's. */
export const createSocket = (url: string) => new WebSocket(url) as unknown as SocketLike;

export function serverUrl(): string {
  if (!MA_URL) throw new Error('MA_URL is not set');
  return MA_URL;
}

/** Signs in as the test user and opens an authenticated connection. */
export async function connect(deviceName = 'Bopsie integration tests') {
  const { account, token } = await signIn(
    { serverUrl: serverUrl(), username: MA_USERNAME, password: MA_PASSWORD, deviceName },
    { createSocket },
  );
  const connection = new MaConnection({ serverUrl: account.serverUrl, token, createSocket });
  await connection.ready();
  return { account, token, connection };
}
