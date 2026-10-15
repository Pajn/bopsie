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
 * Another player the app controls, for the phone's media session to show while the phone plays
 * nothing itself: its notification, lock screen controls and volume keys.
 */
export interface RemoteNowPlaying {
  title: string;
  artist: string | null;
  album: string | null;
  artworkUrl: string | null;
  durationMs: number | null;
  /** Milliseconds into the track at `positionAt`. */
  positionMs: number;
  /** Unix milliseconds. */
  positionAt: number;
  playing: boolean;
  /** 0–100, or null where the player has no volume control. */
  volume: number | null;
}

/** What the media session asks of the player it shows for the app. */
export type RemoteCommand =
  | { command: 'play' | 'pause' | 'stop' | 'next' | 'previous' }
  | { command: 'seek'; positionMs: number }
  | { command: 'volume'; level: number };

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
  /** Shows another player in the media session, or, with null, stops. */
  showRemote(state: RemoteNowPlaying | null): void;
  /** Hears what the media session asks of the other player it shows. */
  onRemoteCommand(listener: (command: RemoteCommand) => void): () => void;
}
