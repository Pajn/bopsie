import { MaConnection, type SocketFactory } from '../api/client';
import { createdToken, loginResult } from '../api/schemas/auth';
import { MIN_SCHEMA_VERSION } from '../api/schemas/server';
import { normalizeServerUrl } from '../api/url';
import { type Account, accountId } from './types';

export class SignInError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'SignInError';
  }
}

export interface SignInInput {
  serverUrl: string;
  username: string;
  password: string;
  /** Shown in the server's list of sign-ins, such as `Bopsie on Android`. */
  deviceName: string;
}

export interface SignInResult {
  account: Account;
  token: string;
}

/**
 * Signs in with a username and password, then trades the session for a
 * long-lived token named after the device. Session tokens slide for 30 days
 * of use but expire after 90 regardless; a long-lived token lasts a year.
 */
export async function signIn(
  input: SignInInput,
  options: { createSocket?: SocketFactory; now?: () => number } = {},
): Promise<SignInResult> {
  const serverUrl = normalizeServerUrl(input.serverUrl);
  const anonymous = new MaConnection({
    serverUrl,
    maxReconnectAttempts: 0,
    ...(options.createSocket && { createSocket: options.createSocket }),
  });
  let session: MaConnection | null = null;
  try {
    await anonymous.ready();
    const info = anonymous.serverInfo;
    if (!info || info.schema_version < MIN_SCHEMA_VERSION) {
      throw new SignInError(
        `This server runs Music Assistant ${info?.server_version ?? 'of an unknown version'}. Bopsie needs 2.7 or later.`,
      );
    }
    const login = await anonymous.command(
      'auth/login',
      { username: input.username, password: input.password, device_name: input.deviceName },
      { schema: loginResult },
    );
    if (!login.success) throw new SignInError(login.error || 'Wrong username or password.');

    session = new MaConnection({
      serverUrl,
      token: login.access_token,
      maxReconnectAttempts: 0,
      ...(options.createSocket && { createSocket: options.createSocket }),
    });
    const token = await session.command(
      'auth/token/create',
      { name: input.deviceName },
      { schema: createdToken },
    );
    // The session token has served its purpose; revoking it keeps the
    // server's list of sign-ins to one entry per device.
    await session.command('auth/logout').catch(() => undefined);

    return {
      token,
      account: {
        id: accountId(info.server_id, login.user.user_id),
        serverUrl,
        serverId: info.server_id,
        userId: login.user.user_id,
        username: login.user.username,
        addedAt: (options.now ?? Date.now)(),
      },
    };
  } finally {
    anonymous.close();
    session?.close();
  }
}
