import type { Player } from '@bopsie/client/api/schemas/player';

/** How long a change may go unconfirmed before the server's state wins. */
export const PENDING_MS = 5000;

/** Joining and leaving a group can take players a while. */
export const MEMBERSHIP_PENDING_MS = 10_000;

/** Fields that say which group a player is in. */
const MEMBERSHIP_FIELDS: ReadonlySet<keyof Player> = new Set(['synced_to', 'active_group']);

/** Fields the app predicts, and the values it predicts for them. */
export type PlayerChanges = Record<string, Partial<Player>>;

/**
 * A change the app has asked for and shows before the server confirms it.
 * `stale` holds the values the server reported before the change, and
 * values asked for earlier: updates still carrying one of them are old news.
 */
export interface Pending {
  playerId: string;
  field: keyof Player;
  value: unknown;
  stale: unknown[];
  until: number;
}

/** Equal values; a field the server leaves out counts as null. */
const same = (a: unknown, b: unknown) => JSON.stringify(a ?? null) === JSON.stringify(b ?? null);

/**
 * Adds predicted changes. A new prediction for a field replaces the one
 * before it, which joins the values that count as stale. Values a player
 * already has are not predicted.
 */
export function addPending(
  players: Player[],
  pending: Pending[],
  changes: PlayerChanges,
  now: number,
  waitMs: number = PENDING_MS,
): Pending[] {
  let next = pending;
  for (const [playerId, fields] of Object.entries(changes)) {
    const player = players.find((p) => p.player_id === playerId);
    if (!player) continue;
    for (const [field, value] of Object.entries(fields)) {
      const key = field as keyof Player;
      const earlier = next.find((p) => p.playerId === playerId && p.field === key);
      // Nothing to wait for when the player already has the value.
      if (!earlier && same(player[key], value)) continue;
      const stale = earlier ? [...earlier.stale, earlier.value] : [player[key]];
      next = [
        ...next.filter((p) => p !== earlier),
        { playerId, field: key, value, stale, until: now + waitMs },
      ];
    }
  }
  return next;
}

/** Drops predictions for these players, as when the server refused the change. */
export function dropPending(pending: Pending[], playerIds: string[]): Pending[] {
  return pending.filter((p) => !playerIds.includes(p.playerId));
}

/**
 * Keeps the predictions the server has not answered yet. One ends when the
 * server reports the predicted value or any value that is not stale, since a
 * prediction can be approximate, or when it has waited too long.
 */
export function settlePending(players: Player[], pending: Pending[], now: number): Pending[] {
  const kept = pending.filter((p) => {
    if (now >= p.until) return false;
    const player = players.find((candidate) => candidate.player_id === p.playerId);
    if (!player) return false;
    const reported = player[p.field];
    return !same(reported, p.value) && p.stale.some((value) => same(value, reported));
  });
  return kept.length === pending.length ? pending : kept;
}

/** Players whose move into or out of a group the server has not confirmed. */
export function joiningOrLeaving(pending: Pending[]): ReadonlySet<string> {
  return new Set(pending.filter((p) => MEMBERSHIP_FIELDS.has(p.field)).map((p) => p.playerId));
}

/** The players as they will be once the pending changes land. */
export function applyPending(players: Player[], pending: Pending[]): Player[] {
  if (pending.length === 0) return players;
  return players.map((player) => {
    const own = pending.filter((p) => p.playerId === player.player_id);
    if (own.length === 0) return player;
    return { ...player, ...Object.fromEntries(own.map((p) => [p.field, p.value])) };
  });
}
