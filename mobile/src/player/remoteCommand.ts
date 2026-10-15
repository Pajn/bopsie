import type { RemoteCommand } from './Player';

const COMMANDS = new Set(['play', 'pause', 'stop', 'next', 'previous', 'seek', 'volume']);

/** A command from the media session for the player it shows for the app. */
export function parseRemoteCommand(json: string): RemoteCommand | null {
  try {
    const event = JSON.parse(json) as {
      type?: string;
      command?: string;
      positionMs?: number;
      level?: number;
    };
    if (event.type !== 'command' || !event.command || !COMMANDS.has(event.command)) return null;
    if (event.command === 'seek') {
      return typeof event.positionMs === 'number'
        ? { command: 'seek', positionMs: event.positionMs }
        : null;
    }
    if (event.command === 'volume') {
      return typeof event.level === 'number' ? { command: 'volume', level: event.level } : null;
    }
    return { command: event.command as 'play' | 'pause' | 'stop' | 'next' | 'previous' };
  } catch {
    return null;
  }
}
