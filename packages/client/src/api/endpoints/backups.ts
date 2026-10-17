import type { MaConnection } from '../client';
import { backup, backupsStatus } from '../schemas/backups';

/** The server's backups, their schedule and the last scheduled run. Admins only. */
export const getBackups = (connection: MaConnection) =>
  connection.command('admin/backups/status', {}, { schema: backupsStatus });

/**
 * Makes a backup of the whole server now, encrypted with `passphrase` when given. It can take
 * a minute for a large library's artwork.
 */
export const createBackup = (connection: MaConnection, passphrase?: string) =>
  connection.command('admin/backups/create', passphrase ? { passphrase } : {}, {
    schema: backup,
    timeoutMs: 600_000,
  });

export const deleteBackup = (connection: MaConnection, name: string) =>
  connection.command<void>('admin/backups/delete', { name });

/**
 * Makes backups by themselves, or stops when `schedule` is null. Hours and days are UTC.
 * `passphrase` encrypts scheduled backups; empty stops that; left out, it stays as it is.
 */
export const setBackupSchedule = (
  connection: MaConnection,
  schedule: { every: 'daily' | 'weekly'; hour: number; weekday: number; keep: number } | null,
  passphrase?: string,
) =>
  connection.command<void>('admin/backups/schedule', {
    ...(schedule ?? {}),
    ...(passphrase !== undefined ? { passphrase } : {}),
  });

/** Restores a backup kept on the server; the server restarts to do so. */
export const restoreStoredBackup = (connection: MaConnection, name: string, passphrase?: string) =>
  connection.command<void>('admin/backups/restore', {
    name,
    ...(passphrase ? { passphrase } : {}),
  });

const base = (serverUrl: string) => serverUrl.replace(/\/+$/, '');

async function failure(response: Response, what: string): Promise<Error> {
  return new Error((await response.text()) || `${what} failed (${response.status})`);
}

/** Downloads backup `name`, over HTTP since it is a file. */
export async function downloadBackup(
  serverUrl: string,
  token: string,
  name: string,
  fetcher: typeof fetch = fetch,
): Promise<Blob> {
  const response = await fetcher(`${base(serverUrl)}/backups/${encodeURIComponent(name)}`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!response.ok) throw await failure(response, 'The download');
  return response.blob();
}

/**
 * Uploads a backup to restore, with its passphrase when it is encrypted. Once the server has
 * checked it, it restarts to restore it.
 */
export async function restoreBackupFile(
  serverUrl: string,
  token: string,
  file: Blob,
  passphrase: string | undefined,
  fetcher: typeof fetch = fetch,
): Promise<void> {
  const response = await fetcher(`${base(serverUrl)}/backups/restore`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${token}`,
      ...(passphrase ? { 'X-Backup-Passphrase': passphrase } : {}),
    },
    body: file,
  });
  if (!response.ok) throw await failure(response, 'The restore');
}
