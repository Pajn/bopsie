/**
 * Speakers for the tests to play to: Sendspin clients opened from Node, as a speaker on the
 * network connects, which the server lists as players and plays to. They take what they are
 * sent and play nothing.
 */

export const USERNAME = process.env.TEST_SERVER_USERNAME ?? 'bopsie';
export const PASSWORD = process.env.TEST_SERVER_PASSWORD ?? 'bopsie-test';

export const SPEAKERS = [
  { id: 'e2e-kitchen', name: 'E2E Kitchen' },
  { id: 'e2e-bedroom', name: 'E2E Bedroom' },
] as const;

async function token(baseUrl: string): Promise<string> {
  const response = await fetch(`${baseUrl}/auth/login`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      credentials: { username: USERNAME, password: PASSWORD },
      device_name: 'Bopsie web smoke tests',
    }),
  });
  if (!response.ok) throw new Error(`Signing in failed: ${response.status}`);
  return ((await response.json()) as { token: string }).token;
}

/** Connects one speaker and resolves once the server has its state, as a ready player. */
function connect(baseUrl: string, auth: string, id: string, name: string): Promise<WebSocket> {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(`${baseUrl.replace(/^http/, 'ws')}/sendspin`);
    const send = (message: object) => socket.send(JSON.stringify(message));
    const support = {
      supported_formats: [{ codec: 'pcm', channels: 2, sample_rate: 48000, bit_depth: 16 }],
      buffer_capacity: 1_048_576,
      supported_commands: ['volume', 'mute'],
    };
    socket.onopen = () => send({ type: 'auth', token: auth, client_id: id });
    socket.onerror = () => reject(new Error(`${name} could not connect`));
    socket.onmessage = (event) => {
      if (typeof event.data !== 'string') return;
      const message = JSON.parse(event.data) as { type: string };
      if (message.type === 'auth_ok') {
        send({
          type: 'client/hello',
          payload: {
            client_id: id,
            name,
            version: 1,
            supported_roles: ['player@v1', 'controller@v1', 'metadata@v1'],
            // A speaker, which every client sees, rather than someone's own phone.
            device_info: {
              product_name: 'Test Speaker',
              manufacturer: 'Bopsie',
              software_version: '0.0.0',
            },
            'player@v1_support': support,
            player_support: support,
          },
        });
      } else if (message.type === 'server/hello') {
        send({ type: 'client/time', payload: { client_transmitted: 1 } });
      } else if (message.type === 'server/time') {
        send({
          type: 'client/state',
          payload: {
            available: true,
            state: 'synchronized',
            player: {
              volume: 40,
              muted: false,
              static_delay_ms: 0,
              required_lead_time_ms: 300,
              min_buffer_ms: 300,
            },
          },
        });
        resolve(socket);
      }
    };
  });
}

/** Connects every speaker; the returned function disconnects them. */
export async function connectSpeakers(baseUrl: string): Promise<() => void> {
  const auth = await token(baseUrl);
  const sockets = await Promise.all(SPEAKERS.map((s) => connect(baseUrl, auth, s.id, s.name)));
  return () => {
    for (const socket of sockets) socket.close();
  };
}
