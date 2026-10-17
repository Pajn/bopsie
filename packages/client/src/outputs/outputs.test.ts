import { type Player, player } from '../api/schemas/player';
import { activeLeaderId, describeOutputs } from './outputs';

function make(id: string, fields: Record<string, unknown> = {}): Player {
  return player.parse({
    player_id: id,
    provider: 'sendspin',
    name: id[0]?.toUpperCase() + id.slice(1),
    available: true,
    playback_state: 'idle',
    volume_level: 50,
    supported_features: ['set_members', 'volume_set'],
    can_group_with: ['sendspin'],
    ...fields,
  });
}

/** Kitchen leads Living room and plays; the phone and Bedroom are on their own. */
const PLAYERS = [
  make('kitchen', {
    playback_state: 'playing',
    group_members: ['kitchen', 'living'],
    group_volume: 62,
    volume_level: 70,
    current_media: { title: 'Dreams' },
  }),
  make('living', { name: 'Living room', synced_to: 'kitchen', volume_level: 54 }),
  make('phone', { name: 'Bopsie on Pixel' }),
  make('bedroom', { playback_state: 'playing', current_media: { title: 'Teardrop' } }),
];

describe('activeLeaderId', () => {
  it('is the phone until the user chooses', () => {
    expect(activeLeaderId(PLAYERS, null, 'phone')).toBe('phone');
  });

  it('follows a chosen player into the group it joined', () => {
    expect(activeLeaderId(PLAYERS, 'living', 'phone')).toBe('kitchen');
  });

  it('follows a member of a group player to the group', () => {
    const players = [make('party', { type: 'group' }), make('den', { active_group: 'party' })];
    expect(activeLeaderId(players, 'den', null)).toBe('party');
  });

  it('falls back to the phone when the chosen player is gone', () => {
    expect(activeLeaderId(PLAYERS, 'attic', 'phone')).toBe('phone');
  });

  it('keeps the choice while the player list loads', () => {
    expect(activeLeaderId(undefined, 'bedroom', 'phone')).toBe('bedroom');
  });

  it('keeps the phone before it has registered', () => {
    expect(activeLeaderId([], null, 'phone')).toBe('phone');
  });

  it('has nothing to control on a phone that cannot play until the user chooses', () => {
    expect(activeLeaderId(PLAYERS, null, null)).toBeNull();
  });
});

describe('describeOutputs', () => {
  it('names a sync group by its members, leader first, with their volumes', () => {
    const { active } = describeOutputs(PLAYERS, 'kitchen', 'phone');
    expect(active).toMatchObject({ id: 'kitchen', name: 'Kitchen + Living room', volume: 62 });
    expect(active?.members).toEqual([
      { id: 'kitchen', name: 'Kitchen', volume: 70, muted: null, leads: true, removable: true },
      {
        id: 'living',
        name: 'Living room',
        volume: 54,
        muted: null,
        leads: false,
        removable: true,
      },
    ]);
  });

  it('shortens the name of a large group', () => {
    const players = [
      make('a', { group_members: ['a', 'b', 'c'] }),
      make('b', { synced_to: 'a' }),
      make('c', { synced_to: 'a' }),
    ];
    expect(describeOutputs(players, 'a', null).active?.name).toBe('A + 2 more');
  });

  it('lets no one remove the last member', () => {
    const { active } = describeOutputs(PLAYERS, 'bedroom', 'phone');
    expect(active?.members).toEqual([
      { id: 'bedroom', name: 'Bedroom', volume: 50, muted: null, leads: true, removable: false },
    ]);
    expect(active?.volume).toBe(50);
  });

  it('says which members are muted, of those that can be', () => {
    const players = [
      make('a', {
        group_members: ['a', 'b', 'c'],
        supported_features: ['set_members', 'volume_mute'],
        volume_muted: true,
      }),
      make('b', { synced_to: 'a', supported_features: ['volume_mute'] }),
      make('c', { synced_to: 'a' }),
    ];
    const { active } = describeOutputs(players, 'a', null);
    expect(active?.members.map((member) => member.muted)).toEqual([true, false, null]);
    // A group is muted member by member.
    expect(active?.muted).toBeNull();
  });

  it('mutes a lone player as its only member', () => {
    const players = [make('a', { supported_features: ['volume_mute'], volume_muted: true })];
    expect(describeOutputs(players, 'a', null).active?.muted).toBe(true);
  });

  it('keeps a group player’s fixed members', () => {
    const players = [
      make('party', {
        type: 'group',
        group_members: ['den', 'hall'],
        static_group_members: ['den'],
      }),
      make('den', { active_group: 'party' }),
      make('hall', { active_group: 'party' }),
    ];
    const { active } = describeOutputs(players, 'party', null);
    expect(active?.name).toBe('Party');
    expect(active?.members.map((member) => [member.id, member.leads, member.removable])).toEqual([
      ['den', false, false],
      ['hall', false, true],
    ]);
  });

  it('lists the phone first, then the rest by name, leaving out the active group', () => {
    const { others } = describeOutputs(PLAYERS, 'kitchen', 'phone');
    expect(others.map((other) => [other.id, other.name, other.status])).toEqual([
      ['phone', 'This phone', 'Idle'],
      ['bedroom', 'Bedroom', 'Playing Teardrop'],
    ]);
  });

  it('puts unavailable players last, even the phone', () => {
    const players = [
      make('attic', { available: false }),
      make('phone', { available: false }),
      make('zone'),
      make('bedroom'),
    ];
    expect(describeOutputs(players, 'bedroom', 'phone').others.map((other) => other.id)).toEqual([
      'zone',
      'phone',
      'attic',
    ]);
  });

  it('shows another group once, through its leader', () => {
    const { others } = describeOutputs(PLAYERS, 'bedroom', 'phone');
    expect(others.map((other) => [other.id, other.name, other.isGroup])).toEqual([
      ['phone', 'This phone', false],
      ['kitchen', 'Kitchen + Living room', true],
    ]);
  });

  it('says what joining would stop', () => {
    const bedroom = describeOutputs(PLAYERS, 'kitchen', 'phone').others.find(
      (other) => other.id === 'bedroom',
    );
    expect(bedroom).toMatchObject({ playing: 'Teardrop', canJoin: true });
  });

  it('offers joining only to lone players the leader can group with', () => {
    const players = [
      ...PLAYERS,
      make('sonos', { provider: 'sonos--1' }),
      make('office', { available: false }),
    ];
    const joinable = describeOutputs(players, 'bedroom', 'phone').others.map((other) => [
      other.id,
      other.canJoin,
    ]);
    expect(joinable).toEqual([
      ['phone', true],
      ['kitchen', false],
      ['sonos', false],
      ['office', false],
    ]);
  });

  it('offers no joining to a leader that cannot group', () => {
    const players = [make('radio', { supported_features: [] }), make('phone')];
    expect(describeOutputs(players, 'radio', 'phone').others[0]?.canJoin).toBe(false);
  });

  it('hides private, hidden, disabled and silent players', () => {
    const players = [
      make('phone'),
      make('other-phone', { private: true }),
      make('hidden', { hide_in_ui: true }),
      make('disabled', { enabled: false }),
      make('screen', { type: 'display' }),
    ];
    expect(describeOutputs(players, 'phone', 'phone').others).toEqual([]);
  });

  it('shows its own phone even though the server marks it private', () => {
    const players = [make('kitchen'), make('phone', { private: true })];
    expect(describeOutputs(players, 'kitchen', 'phone').others[0]?.id).toBe('phone');
  });

  it('has no volume for a group whose members have none', () => {
    const players = [make('tv', { volume_level: null, group_volume: 0 })];
    expect(describeOutputs(players, 'tv', null).active?.volume).toBeNull();
  });
});
