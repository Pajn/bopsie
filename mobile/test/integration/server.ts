import { MaConnection, type SocketLike } from '@bopsie/client/api/client';
import { libraryItems } from '@bopsie/client/api/endpoints/library';
import { signIn } from '@bopsie/client/auth/signIn';

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

/**
 * The URI of the fixture album named `name`. Item ids are the server's to assign, so tests
 * look albums up rather than assume an id.
 */
export async function albumUri(connection: MaConnection, name: string): Promise<string> {
  const albums = await libraryItems(connection, 'albums', { limit: 500, offset: 0 });
  const album = albums.find((item) => item.name === name);
  if (!album) throw new Error(`No album named ${name}`);
  return album.uri;
}
