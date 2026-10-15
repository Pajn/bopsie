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

/** How an AirPlay endpoint reaches its speaker, as a person reads it. */
export interface AirplayRoute {
  /** AirPlay 2 or AirPlay 1, as the last session went, else as one would. */
  protocol: string;
  /** The speaker's clock: PTP and whether it follows ours, or NTP. */
  clock: string;
  /** Whether that clock is as it should be. */
  tone: Tone;
}

const PROTOCOLS: Record<string, string> = { airplay2: 'AirPlay 2', raop: 'AirPlay 1 (RAOP)' };

/** What an AirPlay endpoint's diagnostics say of its protocol and clock. */
export function airplayRoute(diagnostics: unknown): AirplayRoute {
  const d = (diagnostics ?? {}) as {
    plans_airplay2?: boolean;
    process?: {
      route?: { protocol?: string; timing?: string } | null;
      clock?: { mode?: string; state?: string } | null;
    };
  };
  const route = d.process?.route;
  const planned = d.plans_airplay2 === undefined ? null : d.plans_airplay2 ? 'airplay2' : 'raop';
  const protocol = route?.protocol
    ? (PROTOCOLS[route.protocol] ?? route.protocol)
    : planned
      ? `${PROTOCOLS[planned]}, once it plays`
      : 'not known until it plays';
  const clock = d.process?.clock;
  const mode = clock?.mode ?? route?.timing;
  if (mode === 'ptp') {
    if (clock?.state === 'stalled') {
      return { protocol, clock: 'PTP, but it does not follow our clock', tone: 'danger' };
    }
    if (clock?.state === 'probing' || clock?.state === 'cold') {
      return { protocol, clock: 'PTP, settling', tone: 'warning' };
    }
    return { protocol, clock: `PTP${clock?.state ? `, ${clock.state}` : ''}`, tone: 'good' };
  }
  if (mode === 'ntp') return { protocol, clock: 'NTP: no PTP', tone: 'warning' };
  return { protocol, clock: 'not known until it plays', tone: 'muted' };
}
