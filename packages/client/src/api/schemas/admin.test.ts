import { adminDevice, adminGroup, librarySuggestion } from './admin';

test('an admin device reads as the server writes it', () => {
  const parsed = adminDevice.parse({
    id: 'kitchen',
    name: 'Kitchen',
    enabled: true,
    available: true,
    max_volume: 40,
    spotify_name: 'Kitchen (Bopsie)',
    spotify_default_name: 'Kitchen (Bopsie)',
    settings: {
      name: { value: 'Kitchen', layer: 'saved' },
      enabled: { value: true, layer: 'default' },
      max_volume: { value: 40, layer: 'config' },
      spotify_connect: { value: true, layer: 'default' },
      spotify_name: { value: null, layer: 'default' },
    },
    sources: [
      {
        id: 'spotify_connect--kitchen',
        name: 'Spotify',
        connected: true,
        playing: true,
        title: 'Step on You',
        artist: null,
      },
    ],
    members: [
      {
        id: 'kitchen',
        provider: 'airplay',
        name: 'Kitchen',
        available: true,
        volume: 30,
        muted: false,
        model: null,
        manufacturer: null,
        software_version: null,
        app: false,
        native_leader: null,
        facts: {
          mac: 'aabbcc001122',
          serial: null,
          uuid: null,
          address: '10.0.0.2',
          features: '0x4a7ffff7',
          status_flags: null,
        },
        capabilities: {
          formats: [{ sample_rate: 44100, channels: 2, bits: 16 }],
          lead_ms: 2000,
          sync_group: true,
          max_group_size: null,
          volume: true,
          mute: true,
          pairing: 'none',
          native_group: null,
          withdrawn: [],
        },
        quirks: { min_lead_ms: null, timing: 'ptp' },
        quirk_sources: {
          timing: { layer: 'rule', name: 'sonos' },
          min_lead_ms: { layer: 'default' },
        },
        health: {
          level: 'good',
          counts: { started: 2 },
          recent: [{ event: 'started', ago_s: 1.5 }],
        },
        diagnostics: { process: { running: true } },
        disabled: false,
        settings: {
          enabled: { value: true, layer: 'default' },
          password: { value: false, layer: 'default' },
        },
      },
    ],
  });
  expect(parsed.members[0]?.quirk_sources.timing).toEqual({ layer: 'rule', name: 'sonos' });
  expect(parsed.members[0]?.health.recent[0]?.event).toBe('started');
  expect(parsed.settings.max_volume).toEqual({ value: 40, layer: 'config' });
  expect(parsed.sources[0]?.title).toBe('Step on You');
});

test('a group without a plan reads', () => {
  expect(adminGroup.parse({ leader: 'kitchen', members: ['bedroom'], plan: null }).plan).toBeNull();
});

test('a suggestion reads both albums and their sources', () => {
  const side = (id: string, instance: string) => ({
    item_id: id,
    provider: 'library',
    name: 'First Album (Deluxe)',
    uri: `library://album/${id}`,
    media_type: 'album',
    artists: [],
    provider_mappings: [
      { item_id: `held-${id}`, provider_domain: 'spotify', provider_instance: instance },
    ],
  });
  const parsed = librarySuggestion.parse({
    id: 3,
    media_type: 'album',
    a: side('1', 'filesystem_local--abc'),
    b: side('2', 'spotify--def'),
    reason: '3 of 4 tracks are the same, in another edition',
    decision: null,
  });
  expect(parsed.b.provider_mappings[0]?.provider_instance).toBe('spotify--def');
  expect(parsed.a.metadata.images).toEqual([]);
  expect(parsed.decision).toBeNull();
  expect(parsed.a.external_ids).toEqual([]);
});

test("an item's external ids read as kind and value", () => {
  const parsed = librarySuggestion.parse({
    id: 1,
    media_type: 'album',
    a: {
      item_id: '1',
      provider: 'library',
      name: 'A',
      uri: 'library://album/1',
      media_type: 'album',
      external_ids: [['musicbrainz_albumid', '11111111-1111-1111-1111-111111111111']],
    },
    b: {
      item_id: '2',
      provider: 'library',
      name: 'B',
      uri: 'library://album/2',
      media_type: 'album',
    },
    reason: '',
    decision: null,
  });
  expect(parsed.a.external_ids[0]).toEqual([
    'musicbrainz_albumid',
    '11111111-1111-1111-1111-111111111111',
  ]);
});
