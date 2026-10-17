import type { Player } from '../api/schemas/player';

/** Player types that render audio. Displays, visualizers, lights and sources do not. */
const PLAYBACK_TYPES = new Set(['player', 'stereo_pair', 'group']);

export interface Member {
  id: string;
  name: string;
  /** 0–100, or null when the player has no volume control. */
  volume: number | null;
  /** The sync leader. Removing it hands the group to another member. */
  leads: boolean;
  /** False for a group player's fixed members and for the last member left. */
  removable: boolean;
}

/** The group the app controls: the mini player, Now Playing and the queue follow it. */
export interface ActiveGroup {
  /** The leader's id, which is also the queue's id. */
  id: string;
  name: string;
  available: boolean;
  members: Member[];
  volume: number | null;
}

/** A player, or a group shown by its leader, that is not part of the active group. */
export interface OtherOutput {
  id: string;
  name: string;
  available: boolean;
  isPhone: boolean;
  isGroup: boolean;
  /** What it is doing, such as "Playing Teardrop". */
  status: string;
  /** What it is playing, which joining the active group would stop. */
  playing: string | null;
  canJoin: boolean;
}

export interface Outputs {
  active: ActiveGroup | null;
  others: OtherOutput[];
}

const PHONE_NAME = 'This phone';

/** Whether a player belongs in the list at all. Other people's phones are private. */
function listed(player: Player, phoneId: string | null): boolean {
  return (
    player.enabled &&
    !player.hide_in_ui &&
    PLAYBACK_TYPES.has(player.type) &&
    (!player.private || player.player_id === phoneId)
  );
}

/** The player a grouped player plays through: its group player or sync leader. */
function leaderId(player: Player): string {
  return player.active_group ?? player.synced_to ?? player.player_id;
}

/** Member ids, leader first for a sync group. A player on its own is its only member. */
export function memberIds(leader: Player): string[] {
  if (leader.type === 'group') return leader.group_members;
  const followers = leader.group_members.filter((id) => id !== leader.player_id);
  return [leader.player_id, ...followers];
}

function displayName(player: Player, phoneId: string | null): string {
  return player.player_id === phoneId ? PHONE_NAME : player.name;
}

function groupName(leader: Player, players: Map<string, Player>, phoneId: string | null): string {
  if (leader.type === 'group') return leader.name;
  const names = memberIds(leader).map((id) => {
    const member = players.get(id);
    return member ? displayName(member, phoneId) : id;
  });
  if (names.length <= 2) return names.join(' + ');
  return `${names[0]} + ${names.length - 1} more`;
}

function status(player: Player): { status: string; playing: string | null } {
  if (!player.available) return { status: 'Unavailable', playing: null };
  const title = player.current_media?.title ?? null;
  if (player.playback_state === 'playing')
    return { status: title ? `Playing ${title}` : 'Playing', playing: title };
  if (player.playback_state === 'paused')
    return { status: title ? `Paused · ${title}` : 'Paused', playing: null };
  return { status: 'Idle', playing: null };
}

/**
 * Which group the app controls, as the id of its leader. A player the user
 * chose may since have joined another group; control follows it to that
 * group's leader. Without a choice, or when the chosen player is gone, it is
 * the phone. Before the player list has loaded, the choice stands as made.
 */
export function activeLeaderId(
  players: Player[] | undefined,
  chosenId: string | null,
  phoneId: string | null,
): string | null {
  const id = chosenId ?? phoneId;
  if (!players || !id) return id;
  const chosen = players.find((player) => player.player_id === id && player.enabled);
  if (chosen) return leaderId(chosen);
  return chosenId && chosenId !== phoneId ? activeLeaderId(players, null, phoneId) : id;
}

/** Joining needs a leader that groups, and a lone player it can group with. */
function canJoin(player: Player, leader: Player): boolean {
  return (
    player.available &&
    leader.available &&
    player.type !== 'group' &&
    player.group_members.length <= 1 &&
    leader.supported_features.includes('set_members') &&
    (leader.can_group_with.includes(player.player_id) ||
      leader.can_group_with.includes(player.provider))
  );
}

/**
 * What the output sheet shows: the active group with its members, and every
 * other player or group: the phone first and unavailable ones last.
 */
export function describeOutputs(
  players: Player[],
  activeId: string | null,
  phoneId: string | null,
): Outputs {
  const byId = new Map(players.map((player) => [player.player_id, player]));
  const leader = activeId ? byId.get(activeId) : undefined;

  let active: ActiveGroup | null = null;
  if (leader) {
    const ids = memberIds(leader);
    const fixed = new Set(leader.static_group_members);
    const members = ids.flatMap((id): Member[] => {
      const player = byId.get(id);
      if (!player) return [];
      return [
        {
          id,
          name: displayName(player, phoneId),
          volume: player.volume_level ?? null,
          leads: leader.type !== 'group' && id === leader.player_id,
          removable: ids.length > 1 && !fixed.has(id),
        },
      ];
    });
    const anyVolume = members.some((member) => member.volume !== null);
    active = {
      id: leader.player_id,
      name: groupName(leader, byId, phoneId),
      available: leader.available,
      members,
      volume: !anyVolume
        ? null
        : members.length > 1
          ? (leader.group_volume ?? null)
          : (leader.volume_level ?? null),
    };
  }

  const taken = new Set(active?.members.map((member) => member.id));
  if (active) taken.add(active.id);
  const others = players
    .filter(
      (player) =>
        listed(player, phoneId) &&
        !taken.has(player.player_id) &&
        // Members of other groups are shown through their leader.
        leaderId(player) === player.player_id,
    )
    .map((player): OtherOutput => {
      const grouped = player.type === 'group' || player.group_members.length > 1;
      return {
        id: player.player_id,
        name: grouped ? groupName(player, byId, phoneId) : displayName(player, phoneId),
        available: player.available,
        isPhone: player.player_id === phoneId,
        isGroup: grouped,
        ...status(player),
        canJoin: leader ? canJoin(player, leader) : false,
      };
    })
    .sort(
      (a, b) =>
        Number(b.available) - Number(a.available) ||
        Number(b.isPhone) - Number(a.isPhone) ||
        a.name.localeCompare(b.name),
    );

  return { active, others };
}
