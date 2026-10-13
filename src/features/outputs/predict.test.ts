import { type Player, player } from '@/api/schemas/player';
import { predictGroupVolume, predictJoin, predictLeave } from './predict';

const make = (id: string, fields: Record<string, unknown> = {}): Player =>
  player.parse({ player_id: id, provider: 'sendspin', name: id, available: true, ...fields });

/** Kitchen leads Living room and Den. */
const GROUP = [
  make('kitchen', {
    group_members: ['kitchen', 'living', 'den'],
    group_volume: 60,
    volume_level: 60,
  }),
  make('living', { synced_to: 'kitchen', volume_level: 30 }),
  make('den', { synced_to: 'kitchen', volume_level: null }),
  make('patio'),
];

describe('predictGroupVolume', () => {
  it('raises each member part of the way to 100', () => {
    expect(predictGroupVolume(GROUP, 'kitchen', 80)).toEqual({
      kitchen: { group_volume: 80, volume_level: 80 },
      living: { volume_level: 65 },
    });
  });

  it('scales each member toward 0', () => {
    expect(predictGroupVolume(GROUP, 'kitchen', 30)).toEqual({
      kitchen: { group_volume: 30, volume_level: 30 },
      living: { volume_level: 15 },
    });
  });

  it('sets a lone player’s own volume', () => {
    expect(predictGroupVolume(GROUP, 'patio', 25)).toEqual({ patio: { volume_level: 25 } });
  });
});

describe('predictJoin', () => {
  it('syncs the player to the leader, which lists it last', () => {
    expect(predictJoin(GROUP, 'patio', 'kitchen')).toEqual({
      patio: { synced_to: 'kitchen' },
      kitchen: { group_members: ['kitchen', 'living', 'den', 'patio'] },
    });
  });

  it('joins a group player as part of its group', () => {
    const players = [make('party', { type: 'group', group_members: ['den'] }), make('patio')];
    expect(predictJoin(players, 'patio', 'party')).toEqual({
      patio: { active_group: 'party' },
      party: { group_members: ['den', 'patio'] },
    });
  });
});

describe('predictLeave', () => {
  it('drops a follower from the group', () => {
    expect(predictLeave(GROUP, 'den', 'kitchen')).toEqual({
      den: { synced_to: null },
      kitchen: { group_members: ['kitchen', 'living'] },
    });
  });

  it('dissolves a group that is down to its leader', () => {
    const pair = [
      make('kitchen', { group_members: ['kitchen', 'living'] }),
      make('living', { synced_to: 'kitchen' }),
    ];
    expect(predictLeave(pair, 'living', 'kitchen')).toEqual({
      living: { synced_to: null },
      kitchen: { group_members: [] },
    });
  });

  it('hands a departing leader’s group to the next member', () => {
    expect(predictLeave(GROUP, 'kitchen', 'kitchen', 'living')).toEqual({
      kitchen: { group_members: [], synced_to: null },
      living: { synced_to: null, group_members: ['living', 'den'] },
      den: { synced_to: 'living' },
    });
  });
});
