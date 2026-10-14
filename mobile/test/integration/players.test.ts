import { randomUUID } from 'node:crypto';
import type { MaConnection } from '@bopsie/client/api/client';
import {
  getPlayer,
  getQueue,
  joinGroup,
  leaveGroup,
  listPlayers,
  playMedia,
  queueCommand,
  transferQueue,
} from '@bopsie/client/api/endpoints/players';
import { activeLeaderId, describeOutputs } from '@/features/outputs/outputs';
import { registerPlayer, type TestPlayer } from './sendspinPlayer';
import { albumUri, connect, describeServer } from './server';

async function eventually(check: () => Promise<boolean>, timeoutMs = 20_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!(await check())) {
    if (Date.now() > deadline) throw new Error('Condition did not hold in time');
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}

/**
 * Remote control as the output sheet drives it, against two Sendspin players
 * standing in for speakers: one joins the other's group and leaves it, and a
 * queue moves from one to the other.
 */
describeServer('remote control', () => {
  let connection: MaConnection;
  const players: TestPlayer[] = [];
  const kitchen = `bopsie-test-${randomUUID()}`;
  const bedroom = `bopsie-test-${randomUUID()}`;

  beforeAll(async () => {
    const session = await connect();
    connection = session.connection;
    players.push(await registerPlayer(session, kitchen, 'Kitchen'));
    players.push(await registerPlayer(session, bedroom, 'Bedroom'));
  });

  afterAll(async () => {
    for (const id of [kitchen, bedroom]) await queueCommand(connection, id, 'stop').catch(() => {});
    for (const player of players) player.close();
    connection?.close();
  });

  it('lists both players and offers each to join the other', async () => {
    const all = await listPlayers(connection);
    const { active, others } = describeOutputs(all, kitchen, null);
    expect(active).toMatchObject({ id: kitchen, name: 'Kitchen' });
    expect(others.find((other) => other.id === bedroom)).toMatchObject({
      name: 'Bedroom',
      canJoin: true,
    });
  });

  it('joins a player to a playing group and follows control to the leader', async () => {
    await playMedia(connection, kitchen, await albumUri(connection, 'First Album'));
    await joinGroup(connection, bedroom, kitchen);
    await eventually(async () => (await getPlayer(connection, bedroom))?.synced_to === kitchen);

    const all = await listPlayers(connection);
    expect(activeLeaderId(all, bedroom, null)).toBe(kitchen);
    const { active } = describeOutputs(all, kitchen, null);
    expect(active?.name).toBe('Kitchen + Bedroom');
    expect(active?.members.map((member) => [member.id, member.leads])).toEqual([
      [kitchen, true],
      [bedroom, false],
    ]);
  });

  it('removes a member and leaves the rest playing', async () => {
    await leaveGroup(connection, bedroom);
    await eventually(async () => !(await getPlayer(connection, bedroom))?.synced_to);
    const leader = await getPlayer(connection, kitchen);
    expect(leader?.group_members.filter((id) => id !== kitchen)).toEqual([]);
  });

  it('moves the queue to another player, which plays on from the same track', async () => {
    await transferQueue(connection, kitchen, bedroom);
    await eventually(async () => (await getQueue(connection, bedroom))?.items === 3);
    const moved = await getQueue(connection, bedroom);
    expect(moved?.current_item?.name).toContain('Opening');
    expect((await getQueue(connection, kitchen))?.items).toBe(0);
  });
});
