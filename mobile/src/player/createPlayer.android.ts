import { requireNativeModule } from 'expo';
import type { LocalPlayer, PlayerStatus } from './Player';
import { parseRemoteCommand } from './remoteCommand';

interface CratiePlayerModule {
  clientId(): string;
  setup(): Promise<void>;
  connect(json: string): Promise<void>;
  disconnect(): Promise<void>;
  snapshot(): Promise<string>;
  showRemote(json: string | null): Promise<void>;
  addListener(event: 'player', listener: (event: { json: string }) => void): { remove(): void };
}

const STATUSES = new Set<PlayerStatus>([
  'closed',
  'connecting',
  'connected',
  'reconnecting',
  'unauthorized',
]);

function parseStatus(json: string): { status: PlayerStatus; message?: string } | null {
  try {
    const event = JSON.parse(json) as { type?: string; status?: string; message?: string };
    if (event.type !== 'status' || !STATUSES.has(event.status as PlayerStatus)) return null;
    return {
      status: event.status as PlayerStatus,
      ...(event.message ? { message: event.message } : {}),
    };
  } catch {
    return null;
  }
}

export function createPlayer(): LocalPlayer {
  const native = requireNativeModule<CratiePlayerModule>('CratiePlayer');
  return {
    supported: true,
    clientId: () => native.clientId(),
    connect: async (config) => {
      await native.setup();
      await native.connect(JSON.stringify({ ...config, clientId: native.clientId() }));
    },
    disconnect: () => native.disconnect(),
    subscribe: (listener) => {
      const subscription = native.addListener('player', ({ json }) => {
        const parsed = parseStatus(json);
        if (parsed) listener(parsed.status, parsed.message);
      });
      native
        .setup()
        .then(() => native.snapshot())
        .then((json) => {
          const parsed = parseStatus(json);
          if (parsed) listener(parsed.status, parsed.message);
        })
        .catch(() => listener('closed'));
      return () => subscription.remove();
    },
    showRemote: (state) => {
      void native
        .setup()
        .then(() => native.showRemote(state ? JSON.stringify(state) : null))
        .catch(() => {});
    },
    onRemoteCommand: (listener) => {
      const subscription = native.addListener('player', ({ json }) => {
        const command = parseRemoteCommand(json);
        if (command) listener(command);
      });
      return () => subscription.remove();
    },
  };
}
