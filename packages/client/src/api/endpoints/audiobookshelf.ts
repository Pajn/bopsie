import type { MaConnection } from '../client';
import { audiobookshelfStatus } from '../schemas/audiobookshelf';

/** How the server's Audiobookshelf sign-in stands. Admins only. */
export const getAudiobookshelfStatus = (connection: MaConnection) =>
  connection.command('admin/audiobookshelf/status', {}, { schema: audiobookshelfStatus });

/**
 * Signs the server in to the Audiobookshelf server at `url`. The server keeps the tokens it is
 * given, not the password.
 */
export const signInToAudiobookshelf = (
  connection: MaConnection,
  url: string,
  username: string,
  password: string,
) =>
  connection.command(
    'admin/audiobookshelf/login',
    { url, username, password },
    { schema: audiobookshelfStatus },
  );

export const signOutOfAudiobookshelf = (connection: MaConnection) =>
  connection.command<void>('admin/audiobookshelf/sign_out', {});
