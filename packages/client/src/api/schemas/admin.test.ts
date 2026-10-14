import { adminDevice, adminGroup } from './admin';

test('an admin device reads as the server writes it', () => {
  const parsed = adminDevice.parse({
    id: 'kitchen',
    name: 'Kitchen',
    enabled: true,
    available: true,
    max_volume: 40,
    settings: {
      name: { value: 'Kitchen', layer: 'saved' },
      enabled: { value: true, layer: 'default' },
      max_volume: { value: 40, layer: 'config' },
    },
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
});

test('a group without a plan reads', () => {
  expect(adminGroup.parse({ leader: 'kitchen', members: ['bedroom'], plan: null }).plan).toBeNull();
});
