import type { AdminDevice, AdminGroup } from '@bopsie/client/api/schemas/admin';
import type { Player } from '@bopsie/client/api/schemas/player';
import { airplayRoute, describeSource, playerRows, queueOf, worst } from './model';

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

test('a source says what it does', () => {
  const source = {
    id: 'spotify_connect--kitchen',
    name: 'Spotify',
    connected: true,
    playing: true,
    title: 'Teardrop',
    artist: 'Massive Attack',
  };
  expect(describeSource(source)).toBe('Spotify: playing Teardrop by Massive Attack');
  expect(describeSource({ ...source, playing: false, artist: null })).toBe(
    'Spotify: paused on Teardrop',
  );
  expect(describeSource({ ...source, connected: false })).toBe('Spotify: waiting to be picked');
});

describe('airplayRoute', () => {
  it('says AirPlay 2 and that its PTP clock follows ours', () => {
    const route = airplayRoute({
      plans_airplay2: true,
      process: {
        route: { protocol: 'airplay2', timing: 'ptp' },
        clock: { mode: 'ptp', state: 'locked' },
      },
    });
    expect(route).toEqual({ protocol: 'AirPlay 2', clock: 'PTP, locked', tone: 'good' });
  });

  it('says a stalled PTP clock and an AirPlay 1 route are not right', () => {
    expect(
      airplayRoute({
        process: { route: { protocol: 'raop' }, clock: { mode: 'ptp', state: 'stalled' } },
      }),
    ).toEqual({
      protocol: 'AirPlay 1 (RAOP)',
      clock: 'PTP, but it does not follow our clock',
      tone: 'danger',
    });
    expect(airplayRoute({ process: { clock: { mode: 'ntp', state: 'cold' } } }).clock).toBe(
      'NTP: no PTP',
    );
  });

  it('says what it would use before it has played', () => {
    expect(airplayRoute({ plans_airplay2: false, process: { route: null, clock: null } })).toEqual({
      protocol: 'AirPlay 1 (RAOP), once it plays',
      clock: 'not known until it plays',
      tone: 'muted',
    });
  });
});
