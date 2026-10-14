import type { AdminDevice, AdminGroup, AdminSource } from '@bopsie/client/api/schemas/admin';
import type { Player } from '@bopsie/client/api/schemas/player';
import type { Tone } from '@/ui/primitives';

export type Level = 'good' | 'degraded' | 'failing';

/** A device as the players page shows it: what admins see joined with the player clients see. */
export interface PlayerRow {
  id: string;
  name: string;
  available: boolean;
  /** Its endpoints' protocols, in order of preference. */
  protocols: string[];
  /** Its worst endpoint's. */
  health: Level;
  player: Player | undefined;
  /** The device it follows, by name. */
  follows: string | null;
  /** The devices following it, by name. */
  followers: string[];
  /** How the group it leads plays, while it does. */
  strategy: string | null;
}

const LEVELS: Level[] = ['good', 'degraded', 'failing'];

export function healthTone(level: string): Tone {
  return level === 'failing' ? 'danger' : level === 'degraded' ? 'warning' : 'good';
}

export function worst(levels: string[]): Level {
  return levels.reduce<Level>((worst, level) => {
    const known = LEVELS.includes(level as Level) ? (level as Level) : 'good';
    return LEVELS.indexOf(known) > LEVELS.indexOf(worst) ? known : worst;
  }, 'good');
}

/** One row per device, available ones first, then by name. */
export function playerRows(
  devices: AdminDevice[],
  players: Player[],
  groups: AdminGroup[],
): PlayerRow[] {
  const names = new Map(devices.map((device) => [device.id, device.name]));
  const name = (id: string) => names.get(id) ?? id;
  return devices
    .map((device) => {
      const player = players.find((p) => p.player_id === device.id);
      const group = groups.find((g) => g.leader === device.id);
      const leader = groups.find((g) => g.members.includes(device.id))?.leader;
      return {
        id: device.id,
        name: device.name,
        available: device.available,
        protocols: device.members.map((member) => member.provider),
        health: worst(device.members.map((member) => member.health.level)),
        player,
        follows: leader ? name(leader) : (player?.synced_to ?? null),
        followers: (group?.members ?? []).map(name),
        strategy: group?.plan?.strategy ?? null,
      };
    })
    .sort((a, b) =>
      a.available === b.available ? a.name.localeCompare(b.name) : a.available ? -1 : 1,
    );
}

/** The queue a player's controls act on: its leader's while it follows one. */
export function queueOf(row: PlayerRow): string {
  return row.player?.synced_to ?? row.id;
}

/** What a source is doing, in a line: such as `Spotify: playing Teardrop by Massive Attack`. */
export function describeSource(source: AdminSource): string {
  if (!source.connected) return `${source.name}: waiting to be picked`;
  const what = source.title
    ? `${source.title}${source.artist ? ` by ${source.artist}` : ''}`
    : 'nothing yet';
  return `${source.name}: ${source.playing ? 'playing' : 'paused on'} ${what}`;
}
