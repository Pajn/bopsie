import { device } from 'detox';

/** The test server as seen from the test process. See scripts/test-server.sh. */
export const SERVER_URL = process.env.TEST_SERVER_URL ?? 'http://127.0.0.1:18095';

/**
 * The same server as seen from the app. The Android emulator reaches the
 * host at 10.0.2.2; the iOS simulator shares the host's loopback.
 */
export const APP_SERVER_URL =
  process.env.E2E_APP_SERVER_URL ??
  (device.getPlatform() === 'android'
    ? SERVER_URL.replace('127.0.0.1', '10.0.2.2').replace('localhost', '10.0.2.2')
    : SERVER_URL);
export const USERNAME = process.env.TEST_SERVER_USERNAME ?? 'bopsie';
export const PASSWORD = process.env.TEST_SERVER_PASSWORD ?? 'bopsie-test';

let token: string | null = null;

/** Calls a command on the server as the test user, over its HTTP API. */
export async function api<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!token) {
    const login = await fetch(`${SERVER_URL}/auth/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        provider_id: 'builtin',
        credentials: { username: USERNAME, password: PASSWORD },
        device_name: 'bopsie-e2e',
      }),
    });
    token = ((await login.json()) as { token: string }).token;
  }
  const response = await fetch(`${SERVER_URL}/api`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
    body: JSON.stringify({ command, args }),
  });
  if (!response.ok) throw new Error(`${command} failed with ${response.status}`);
  return (await response.json()) as T;
}

/** Polls until `check` holds, for assertions on the server's view of the app. */
export async function eventually(check: () => Promise<boolean>, timeoutMs = 20_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!(await check())) {
    if (Date.now() > deadline) throw new Error('Condition did not hold in time');
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}
