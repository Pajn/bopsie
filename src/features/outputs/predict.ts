import type { Player } from '@/api/schemas/player';
import { memberIds } from './outputs';
import type { PlayerChanges } from './pending';

/**
 * What the server will report after each command, so the sheet can show it
 * at once. Volumes the server derives itself are approximations; the server's
 * own values replace them as they arrive.
 */

/**
 * One member's volume. A group's volume is its loudest member's, so the
 * group follows when this member is or becomes the loudest.
 */
export function predictVolume(
  players: Player[],
  leaderId: string,
  playerId: string,
  level: number,
): PlayerChanges {
  const target = Math.round(level);
  const changes: PlayerChanges = { [playerId]: { volume_level: target } };
  const leader = players.find((p) => p.player_id === leaderId);
  const ids = leader ? memberIds(leader) : [];
  if (!ids.includes(playerId) || ids.length <= 1) return changes;
  const levels = ids.map((id) =>
    id === playerId ? target : players.find((p) => p.player_id === id)?.volume_level,
  );
  const loudest = Math.max(...levels.filter((v): v is number => typeof v === 'number'));
  return { ...changes, [leaderId]: { ...changes[leaderId], group_volume: loudest } };
}

/**
 * The server keeps the members' balance around the loudest member: raising
 * the group moves each member part of the way to 100, lowering it scales
 * each toward 0.
 */
export function predictGroupVolume(
  players: Player[],
  leaderId: string,
  level: number,
): PlayerChanges {
  const byId = new Map(players.map((p) => [p.player_id, p]));
  const leader = byId.get(leaderId);
  if (!leader) return {};
  const target = Math.round(level);
  const ids = memberIds(leader);
  if (ids.length <= 1) return { [leaderId]: { volume_level: target } };

  const changes: PlayerChanges = { [leaderId]: { group_volume: target } };
  const current = leader.group_volume ?? null;
  if (current === null) return changes;
  for (const id of ids) {
    const volume = byId.get(id)?.volume_level;
    if (volume === null || volume === undefined) continue;
    const next =
      target >= current
        ? volume + ((100 - volume) * (target - current)) / Math.max(1, 100 - current)
        : (volume * target) / Math.max(1, current);
    changes[id] = { ...changes[id], volume_level: Math.round(Math.min(100, Math.max(0, next))) };
  }
  return changes;
}

/** `playerId` follows the leader, which lists it as a member. */
export function predictJoin(players: Player[], playerId: string, leaderId: string): PlayerChanges {
  const leader = players.find((p) => p.player_id === leaderId);
  if (!leader) return {};
  return {
    [playerId]: leader.type === 'group' ? { active_group: leaderId } : { synced_to: leaderId },
    [leaderId]: { group_members: [...memberIds(leader), playerId] },
  };
}

/**
 * `playerId` leaves its group. A departing sync leader hands the group to
 * `nextLeaderId`; the server may pick another, which its updates then show.
 */
export function predictLeave(
  players: Player[],
  playerId: string,
  leaderId: string,
  nextLeaderId?: string,
): PlayerChanges {
  const leader = players.find((p) => p.player_id === leaderId);
  if (!leader) return {};
  const remaining = memberIds(leader).filter((id) => id !== playerId);
  if (leader.type === 'group') {
    return {
      [playerId]: { active_group: null },
      [leaderId]: { group_members: remaining },
    };
  }
  if (playerId !== leaderId) {
    return {
      [playerId]: { synced_to: null },
      [leaderId]: { group_members: remaining.length > 1 ? remaining : [] },
    };
  }
  const next = nextLeaderId ?? remaining[0];
  if (!next) return {};
  const followers = remaining.filter((id) => id !== next);
  const changes: PlayerChanges = {
    [playerId]: { group_members: [], synced_to: null },
    [next]: { synced_to: null, group_members: followers.length ? [next, ...followers] : [] },
  };
  for (const id of followers) changes[id] = { synced_to: next };
  return changes;
}
