import { downloadBackup, restoreBackupFile } from './backups';

type Call = { url: string; init: RequestInit | undefined };

function recording(response: () => Response) {
  const calls: Call[] = [];
  const fetcher = (async (url: string, init?: RequestInit) => {
    calls.push({ url, init });
    return response();
  }) as typeof fetch;
  return { calls, fetcher };
}

test('a backup downloads with the token', async () => {
  const { calls, fetcher } = recording(() => new Response('bytes', { status: 200 }));
  const blob = await downloadBackup('http://m:8095/', 'token', 'bopsie-1.tar.zst', fetcher);
  expect(await blob.text()).toBe('bytes');
  expect(calls[0]?.url).toBe('http://m:8095/backups/bopsie-1.tar.zst');
  const headers = (calls[0]?.init?.headers ?? {}) as Record<string, string>;
  expect(headers.Authorization).toBe('Bearer token');
});

test('a backup is uploaded raw, with its passphrase in a header', async () => {
  const { calls, fetcher } = recording(() => new Response('{}', { status: 202 }));
  await restoreBackupFile('http://m', 'token', new Blob(['x']), 'secret', fetcher);
  expect(calls[0]?.url).toBe('http://m/backups/restore');
  expect(calls[0]?.init?.method).toBe('POST');
  const headers = (calls[0]?.init?.headers ?? {}) as Record<string, string>;
  expect(headers['X-Backup-Passphrase']).toBe('secret');
});

test('a refused restore says why', async () => {
  const { fetcher } = recording(
    () => new Response('the passphrase does not open the backup', { status: 401 }),
  );
  await expect(restoreBackupFile('http://m', 't', new Blob([]), 'x', fetcher)).rejects.toThrow(
    'the passphrase does not open the backup',
  );
});
