import { type Player, player } from '@bopsie/client/api/schemas/player';
import {
  addPending,
  applyPending,
  dropPending,
  joiningOrLeaving,
  MEMBERSHIP_PENDING_MS,
  PENDING_MS,
  settlePending,
} from './pending';

const make = (id: string, fields: Record<string, unknown> = {}): Player =>
  player.parse({ player_id: id, provider: 'sendspin', name: id, available: true, ...fields });

const volume = (players: Player[], id: string) =>
  players.find((p) => p.player_id === id)?.volume_level;

describe('pending changes', () => {
  const server = [make('kitchen', { volume_level: 40 }), make('living', { volume_level: 20 })];

  it('shows a change before the server confirms it', () => {
    const pending = addPending(server, [], { kitchen: { volume_level: 70 } }, 0);
    expect(volume(applyPending(server, pending), 'kitchen')).toBe(70);
    expect(volume(applyPending(server, pending), 'living')).toBe(20);
  });

  it('holds the change through updates that still carry the old value', () => {
    const pending = addPending(server, [], { kitchen: { volume_level: 70 } }, 0);
    expect(settlePending(server, pending, 100)).toBe(pending);
  });

  it('ends once the server reports the value', () => {
    const pending = addPending(server, [], { kitchen: { volume_level: 70 } }, 0);
    const confirmed = [make('kitchen', { volume_level: 70 }), server[1] as Player];
    expect(settlePending(confirmed, pending, 100)).toEqual([]);
  });

  it('gives way to any new value the server reports, for approximate predictions', () => {
    const pending = addPending(server, [], { kitchen: { volume_level: 70 } }, 0);
    const actual = [make('kitchen', { volume_level: 66 }), server[1] as Player];
    expect(settlePending(actual, pending, 100)).toEqual([]);
  });

  it('treats a value asked for earlier as stale while a later one is pending', () => {
    let pending = addPending(server, [], { kitchen: { volume_level: 60 } }, 0);
    pending = addPending(server, pending, { kitchen: { volume_level: 80 } }, 50);
    const first = [make('kitchen', { volume_level: 60 }), server[1] as Player];
    const kept = settlePending(first, pending, 100);
    expect(volume(applyPending(first, kept), 'kitchen')).toBe(80);
  });

  it('lets the server win when it never confirms', () => {
    const pending = addPending(server, [], { kitchen: { volume_level: 70 } }, 0);
    expect(settlePending(server, pending, PENDING_MS)).toEqual([]);
  });

  it('drops a refused change at once', () => {
    const pending = addPending(server, [], { kitchen: { volume_level: 70 } }, 0);
    expect(dropPending(pending, ['kitchen'])).toEqual([]);
  });

  it('predicts lists and cleared fields', () => {
    const players = [make('kitchen'), make('living', { synced_to: 'kitchen' })];
    const pending = addPending(
      players,
      [],
      { kitchen: { group_members: ['kitchen', 'den'] }, living: { synced_to: null } },
      0,
    );
    const shown = applyPending(players, pending);
    expect(shown[0]?.group_members).toEqual(['kitchen', 'den']);
    expect(shown[1]?.synced_to).toBeNull();
  });

  it('waits longer for a slow change when asked', () => {
    const pending = addPending(
      server,
      [],
      { kitchen: { synced_to: 'living' } },
      0,
      MEMBERSHIP_PENDING_MS,
    );
    expect(settlePending(server, pending, PENDING_MS)).toBe(pending);
    expect(settlePending(server, pending, MEMBERSHIP_PENDING_MS)).toEqual([]);
  });

  it('names the players joining or leaving a group, not those changing volume', () => {
    const pending = addPending(
      server,
      [],
      { kitchen: { synced_to: 'living' }, living: { volume_level: 5, group_members: ['living'] } },
      0,
    );
    expect([...joiningOrLeaving(pending)]).toEqual(['kitchen']);
  });

  it('waits for nothing when the player already has the value', () => {
    expect(addPending(server, [], { kitchen: { synced_to: null, volume_level: 40 } }, 0)).toEqual(
      [],
    );
  });
});
