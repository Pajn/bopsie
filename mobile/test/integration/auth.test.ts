import { MaConnection } from '@bopsie/client/api/client';
import { SignInError, signIn } from '@bopsie/client/auth/signIn';
import { connect, createSocket, describeServer, MA_USERNAME, serverUrl } from './server';

describeServer('sign-in', () => {
  it('signs in with a password and keeps a long-lived token that authenticates', async () => {
    const { account, connection } = await connect();
    expect(account.username).toBe(MA_USERNAME);
    expect(account.id).toBe(`${account.serverId}#${account.userId}`);
    const me = await connection.command<{ username: string }>('auth/me');
    expect(me.username).toBe(MA_USERNAME);
    connection.close();
  });

  it('reports a wrong password', async () => {
    await expect(
      signIn(
        {
          serverUrl: serverUrl(),
          username: MA_USERNAME,
          password: 'wrong-password',
          deviceName: 'x',
        },
        { createSocket },
      ),
    ).rejects.toBeInstanceOf(SignInError);
  });

  it('marks a connection with a revoked token unauthorized', async () => {
    const { token, connection } = await connect();
    // The server closes the socket as it revokes the token, before answering.
    await connection.command('auth/logout').catch(() => undefined);
    connection.close();

    const revoked = new MaConnection({ serverUrl: serverUrl(), token, createSocket });
    await expect(revoked.ready()).rejects.toMatchObject({ isUnauthorized: true });
    expect(revoked.status).toBe('unauthorized');
  });
});
