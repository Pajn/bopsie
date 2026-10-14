import type { AdminDevice, AdminGroup } from '@bopsie/client/api/schemas/admin';
import type { Player } from '@bopsie/client/api/schemas/player';
import { playerRows, queueOf, worst } from './model';

function device(id: string, name: string, available: boolean, levels: string[]): AdminDevice {
  return {
    id,
    name,
    enabled: true,
    available,
    members: levels.map((level, i) => ({
      id: `${id}-${i}`,
      provider: i === 0 ? 'airplay' : 'sendspin',
      health: { level, counts: {}, recent: [] },
    })),
  } as unknown as AdminDevice;
}

const player = (id: string, synced_to: string | null = null) =>
  ({ player_id: id, synced_to }) as unknown as Player;

test('the worst level wins', () => {
  expect(worst([])).toBe('good');
  expect(worst(['good', 'degraded'])).toBe('degraded');
  expect(worst(['failing', 'degraded', 'something new'])).toBe('failing');
});

test('rows join devices, players and groups', () => {
  const groups: AdminGroup[] = [
    {
      leader: 'kitchen',
      members: ['bedroom'],
      plan: {
        strategy: 'airplay_direct',
        carriers: [],
        sample_rate: 44100,
        channels: 2,
        lead_ms: 2000,
        dropped: [],
      },
    },
  ];
  const rows = playerRows(
    [
      device('old', 'Attic', false, ['good']),
      device('kitchen', 'Kitchen', true, ['good', 'degraded']),
      device('bedroom', 'Bedroom', true, ['good']),
    ],
    [player('kitchen'), player('bedroom', 'kitchen')],
    groups,
  );
  expect(rows.map((r) => r.id)).toEqual(['bedroom', 'kitchen', 'old']);
  const [bedroom, kitchen] = rows;
  expect(kitchen?.protocols).toEqual(['airplay', 'sendspin']);
  expect(kitchen?.health).toBe('degraded');
  expect(kitchen?.followers).toEqual(['Bedroom']);
  expect(kitchen?.strategy).toBe('airplay_direct');
  expect(bedroom?.follows).toBe('Kitchen');
  expect(bedroom && queueOf(bedroom)).toBe('kitchen');
});
