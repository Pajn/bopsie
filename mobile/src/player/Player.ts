export type PlayerStatus =
  | 'unsupported'
  | 'closed'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'unauthorized';

export interface PlayerConfig {
  /** The Music Assistant origin the app connected to. */
  serverUrl: string;
  /** The `ws(s)://…/sendspin` route on that origin. */
  url: string;
  token: string;
  /** How the phone is named in Music Assistant's player list. */
  name: string;
  softwareVersion: string;
}

/**
 * The phone as a Music Assistant player. The native side owns the Sendspin
 * connection and audio; this is how the app starts and observes it. What
 * plays, and transport, go through the Music Assistant API on the player's
 * queue, whose id is `clientId`.
 */
export interface LocalPlayer {
  readonly supported: boolean;
  /** Stable per install. Null where playback is not supported. */
  clientId(): string | null;
  connect(config: PlayerConfig): Promise<void>;
  disconnect(): Promise<void>;
  subscribe(listener: (status: PlayerStatus, message?: string) => void): () => void;
}
