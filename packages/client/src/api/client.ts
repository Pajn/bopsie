import type * as z from 'zod/mini';
import { CommandError, NetworkError, SchemaError, SetupRequiredError } from './errors';
import { type ServerInfo, serverInfo } from './schemas/server';
import { socketUrl } from './url';

/** The part of the WebSocket API the client uses, so tests can supply a fake. */
export interface SocketLike {
  onopen: (() => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
  onclose: ((event: { code: number; reason: string }) => void) | null;
  onerror: ((event: unknown) => void) | null;
  send(data: string): void;
  close(code?: number, reason?: string): void;
}

export type SocketFactory = (url: string) => SocketLike;

export type ConnectionStatus =
  /** Opening the socket, waiting for server info, or authenticating. */
  | 'connecting'
  /** Commands can be sent. */
  | 'ready'
  /** The connection dropped and a retry is scheduled. */
  | 'reconnecting'
  /** The server rejected the token. Nothing more happens until a new sign-in. */
  | 'unauthorized'
  /** Retries are exhausted or the connection was closed on purpose. */
  | 'closed';

export interface MaEvent {
  event: string;
  object_id: string | null;
  data: unknown;
}

export interface ConnectionOptions {
  serverUrl: string;
  /** Authenticates the connection. Without one, only unauthenticated commands such as `auth/login` work. */
  token?: string;
  createSocket?: SocketFactory;
  /** How long a command may wait for the connection and then for its answer. */
  commandTimeoutMs?: number;
  /** Reconnection attempts after an unexpected close. Unbounded retries drain the battery. */
  maxReconnectAttempts?: number;
  locale?: string;
}

interface CommandOptions<T> {
  schema?: z.ZodMiniType<T>;
  timeoutMs?: number;
}

interface Pending {
  command: string;
  partial: unknown[];
  resolve: (value: unknown) => void;
  reject: (error: unknown) => void;
  timer: ReturnType<typeof setTimeout>;
}

const AUTH_MESSAGE_ID = 'auth';

const defaultSocketFactory: SocketFactory = (url) => new WebSocket(url) as unknown as SocketLike;

/** Backoff before reconnection attempt `attempt` (0-based): 1, 2, 4, 8, 16, then 30 seconds. */
export function reconnectDelayMs(attempt: number): number {
  return Math.min(30_000, 1000 * 2 ** attempt);
}

/**
 * A connection to Music Assistant's WebSocket command API at `/ws`.
 *
 * The server opens with its server info. With a token, the client then sends
 * `auth` and is ready once that succeeds; without one it is ready at once.
 * Commands sent before then wait. Results are matched to commands by
 * `message_id`; streaming commands answer in several `partial` batches, which
 * are concatenated. Server events arrive on the same socket once
 * authenticated and go to every `onEvent` listener.
 */
export class MaConnection {
  readonly url: string;
  private readonly createSocket: SocketFactory;
  private readonly commandTimeoutMs: number;
  private readonly maxReconnectAttempts: number;

  private socket: SocketLike | null = null;
  private nextId = 1;
  private pending = new Map<string, Pending>();
  private waiting: Array<{ resolve: () => void; reject: (error: unknown) => void }> = [];
  private eventListeners = new Set<(event: MaEvent) => void>();
  private statusListeners = new Set<(status: ConnectionStatus) => void>();
  private reconnectAttempt = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private wasReady = false;

  status: ConnectionStatus = 'closed';
  serverInfo: ServerInfo | null = null;

  constructor(private readonly options: ConnectionOptions) {
    this.url = socketUrl(options.serverUrl, '/ws');
    this.createSocket = options.createSocket ?? defaultSocketFactory;
    this.commandTimeoutMs = options.commandTimeoutMs ?? 15_000;
    this.maxReconnectAttempts = options.maxReconnectAttempts ?? 8;
  }

  connect(): void {
    if (this.socket || this.status === 'unauthorized') return;
    this.setStatus('connecting');
    const socket = this.createSocket(this.url);
    this.socket = socket;
    socket.onmessage = (event) => {
      if (this.socket === socket) this.receive(event.data);
    };
    socket.onclose = (event) => {
      if (this.socket === socket) this.dropped(new Error(event.reason || `closed (${event.code})`));
    };
    socket.onerror = () => {
      // A close follows every error; it carries the outcome.
    };
  }

  /**
   * Connects again now: after giving up, or instead of waiting out a retry's backoff, as when
   * the app comes back to the foreground or the network returns. It gets a fresh set of
   * retries. Does nothing while connected, or after the server refused the token.
   */
  retry(): void {
    if (this.socket || this.status === 'unauthorized') return;
    this.clearReconnect();
    this.reconnectAttempt = 0;
    this.connect();
  }

  /** Closes for good: no reconnection, and waiting commands fail. */
  close(): void {
    this.clearReconnect();
    const socket = this.socket;
    this.socket = null;
    socket?.close(1000, 'client closing');
    this.failAll(new NetworkError(this.url, new Error('connection closed')));
    this.setStatus('closed');
  }

  /** Resolves once commands can be sent; rejects if the connection gives up first. */
  ready(): Promise<void> {
    if (this.status === 'ready') return Promise.resolve();
    if (this.status === 'unauthorized')
      return Promise.reject(
        new CommandError('auth', 23, 'The server no longer accepts this sign-in.'),
      );
    if (this.status === 'closed') this.connect();
    return new Promise((resolve, reject) => this.waiting.push({ resolve, reject }));
  }

  async command<T = unknown>(
    command: string,
    args: Record<string, unknown> = {},
    options: CommandOptions<T> = {},
  ): Promise<T> {
    const timeoutMs = options.timeoutMs ?? this.commandTimeoutMs;
    await withTimeout(this.ready(), timeoutMs, () => new NetworkError(this.url, 'timed out'));
    const result = await this.send(command, args, timeoutMs);
    if (!options.schema) return result as T;
    const parsed = options.schema.safeParse(result);
    if (!parsed.success) throw new SchemaError(command, parsed.error.issues);
    return parsed.data;
  }

  onEvent(listener: (event: MaEvent) => void): () => void {
    this.eventListeners.add(listener);
    return () => void this.eventListeners.delete(listener);
  }

  onStatus(listener: (status: ConnectionStatus) => void): () => void {
    this.statusListeners.add(listener);
    return () => void this.statusListeners.delete(listener);
  }

  private send(command: string, args: Record<string, unknown>, timeoutMs: number) {
    const socket = this.socket;
    if (!socket) return Promise.reject(new NetworkError(this.url, 'not connected'));
    const messageId = String(this.nextId++);
    return new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(messageId);
        reject(new NetworkError(this.url, `${command} timed out`));
      }, timeoutMs);
      this.pending.set(messageId, { command, partial: [], resolve, reject, timer });
      socket.send(JSON.stringify({ message_id: messageId, command, args }));
    });
  }

  private receive(data: unknown): void {
    if (typeof data !== 'string') return;
    let message: Record<string, unknown>;
    try {
      message = JSON.parse(data) as Record<string, unknown>;
    } catch {
      return;
    }

    if ('server_version' in message) {
      this.receivedServerInfo(message);
      return;
    }
    if (typeof message.event === 'string') {
      const event: MaEvent = {
        event: message.event,
        object_id: typeof message.object_id === 'string' ? message.object_id : null,
        data: message.data,
      };
      for (const listener of this.eventListeners) listener(event);
      return;
    }

    const messageId = message.message_id;
    if (messageId === 'connection' && message.error_code === 503) {
      this.giveUp('closed', new SetupRequiredError());
      return;
    }
    if (messageId === AUTH_MESSAGE_ID) {
      this.receivedAuth(message);
      return;
    }
    if (typeof messageId !== 'string') return;
    const pending = this.pending.get(messageId);
    if (!pending) return;

    if ('error_code' in message) {
      this.settle(messageId);
      pending.reject(
        new CommandError(
          pending.command,
          Number(message.error_code),
          typeof message.details === 'string' ? message.details : undefined,
        ),
      );
      return;
    }
    if (message.partial === true) {
      if (Array.isArray(message.result)) pending.partial.push(...message.result);
      return;
    }
    this.settle(messageId);
    pending.resolve(
      pending.partial.length > 0 && Array.isArray(message.result)
        ? [...pending.partial, ...message.result]
        : message.result,
    );
  }

  private receivedServerInfo(message: Record<string, unknown>): void {
    const parsed = serverInfo.safeParse(message);
    if (!parsed.success) {
      this.giveUp('closed', new SchemaError('server info', parsed.error.issues));
      return;
    }
    this.serverInfo = parsed.data;
    if (!this.options.token) {
      this.becameReady();
      return;
    }
    this.socket?.send(
      JSON.stringify({
        message_id: AUTH_MESSAGE_ID,
        command: 'auth',
        args: {
          token: this.options.token,
          ...(this.options.locale && { locale: this.options.locale }),
        },
      }),
    );
  }

  private receivedAuth(message: Record<string, unknown>): void {
    if ('error_code' in message) {
      const error = new CommandError(
        'auth',
        Number(message.error_code),
        typeof message.details === 'string' ? message.details : undefined,
      );
      this.giveUp(error.isUnauthorized ? 'unauthorized' : 'closed', error);
      return;
    }
    this.becameReady();
  }

  private becameReady(): void {
    this.reconnectAttempt = 0;
    this.wasReady = true;
    this.setStatus('ready');
    const waiting = this.waiting;
    this.waiting = [];
    for (const waiter of waiting) waiter.resolve();
  }

  private dropped(cause: unknown): void {
    this.socket = null;
    // Commands in flight cannot be answered on a new socket.
    this.failInFlight(new NetworkError(this.url, cause));
    const retry = this.wasReady || this.reconnectAttempt > 0 || this.waiting.length > 0;
    if (!retry || this.reconnectAttempt >= this.maxReconnectAttempts) {
      this.failAll(new NetworkError(this.url, cause));
      this.setStatus('closed');
      return;
    }
    this.setStatus('reconnecting');
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, reconnectDelayMs(this.reconnectAttempt));
    this.reconnectAttempt++;
  }

  private giveUp(status: 'unauthorized' | 'closed', error: unknown): void {
    this.clearReconnect();
    const socket = this.socket;
    this.socket = null;
    socket?.close(1000, 'client closing');
    this.failAll(error);
    this.setStatus(status);
  }

  private settle(messageId: string): void {
    const pending = this.pending.get(messageId);
    if (pending) clearTimeout(pending.timer);
    this.pending.delete(messageId);
  }

  private failInFlight(error: unknown): void {
    const pending = [...this.pending.values()];
    this.pending.clear();
    for (const entry of pending) {
      clearTimeout(entry.timer);
      entry.reject(error);
    }
  }

  private failAll(error: unknown): void {
    this.failInFlight(error);
    const waiting = this.waiting;
    this.waiting = [];
    for (const waiter of waiting) waiter.reject(error);
  }

  private clearReconnect(): void {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
  }

  private setStatus(status: ConnectionStatus): void {
    if (this.status === status) return;
    this.status = status;
    for (const listener of this.statusListeners) listener(status);
  }
}

function withTimeout<T>(promise: Promise<T>, ms: number, error: () => Error): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(error()), ms);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (reason) => {
        clearTimeout(timer);
        reject(reason);
      },
    );
  });
}
