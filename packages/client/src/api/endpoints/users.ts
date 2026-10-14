import * as z from 'zod/mini';
import type { MaConnection } from '../client';
import { authToken, managedUser } from '../schemas/users';

/** Every user. Needs the `users.manage` scope. */
export const listUsers = (connection: MaConnection) =>
  connection.command('auth/users', {}, { schema: z.array(managedUser) });

export const createUser = (
  connection: MaConnection,
  user: { username: string; password: string; role: 'admin' | 'user'; displayName?: string },
) =>
  connection.command(
    'auth/user/create',
    {
      username: user.username,
      password: user.password,
      role: user.role,
      ...(user.displayName ? { display_name: user.displayName } : {}),
    },
    { schema: managedUser },
  );

/** Changes a user; yourself unless `userId` names another. Fields left out stay. */
export const updateUser = (
  connection: MaConnection,
  changes: {
    userId?: string;
    username?: string;
    displayName?: string;
    password?: string;
    role?: 'admin' | 'user';
  },
) =>
  connection.command(
    'auth/user/update',
    {
      ...(changes.userId !== undefined && { user_id: changes.userId }),
      ...(changes.username !== undefined && { username: changes.username }),
      ...(changes.displayName !== undefined && { display_name: changes.displayName }),
      ...(changes.password !== undefined && { password: changes.password }),
      ...(changes.role !== undefined && { role: changes.role }),
    },
    { schema: managedUser },
  );

export const setUserEnabled = (connection: MaConnection, userId: string, enabled: boolean) =>
  connection.command<void>(enabled ? 'auth/user/enable' : 'auth/user/disable', {
    user_id: userId,
  });

export const deleteUser = (connection: MaConnection, userId: string) =>
  connection.command<void>('auth/user/delete', { user_id: userId });

/** A user's sign-ins, newest first; yours unless `userId` names another. */
export const listTokens = (connection: MaConnection, userId?: string) =>
  connection.command('auth/tokens', userId === undefined ? {} : { user_id: userId }, {
    schema: z.array(authToken),
  });

/** Revokes a sign-in, closing every connection that uses it. */
export const revokeToken = (connection: MaConnection, tokenId: string) =>
  connection.command<void>('auth/token/revoke', { token_id: tokenId });
