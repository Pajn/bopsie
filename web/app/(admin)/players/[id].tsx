import type { AdminEndpoint, AdminGroup, Format } from '@bopsie/client/api/schemas/admin';
import { useLocalSearchParams } from 'expo-router';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useDevices, useGroups, usePlayers } from '@/data/admin';
import JsonTree from '@/debug/JsonTree';
import { useTreeColors } from '@/debug/treeColors';
import { airplayRoute, healthTone, playerRows } from '@/features/players/model';
import { PlayerCard } from '@/features/players/PlayerCard';
import { PlayerSettings } from '@/features/players/PlayerSettings';
import { describeError } from '@/lib/errors';
import { Disclosure } from '@/ui/controls';
import { Badge, Card, Facts, InlineError, Page, Stack, Text } from '@/ui/primitives';

type Json = Parameters<typeof JsonTree>[0]['value'];

/** JSON-safe: what the tree component can be handed. */
const json = (value: unknown): Json => JSON.parse(JSON.stringify(value ?? null)) as Json;

const formatText = (f: Format) => `${f.sample_rate / 1000} kHz / ${f.bits}-bit / ${f.channels} ch`;

const dash = (value: string | number | null | undefined) =>
  value === null || value === undefined || value === '' ? '–' : String(value);

function source(source: { layer: string; name?: string | undefined } | undefined): string {
  if (!source) return '';
  return source.layer === 'rule' && source.name ? `rule ${source.name}` : source.layer;
}

/** Which AirPlay a speaker is reached over, and whether its PTP clock follows ours. */
function AirplayRouteRows({ diagnostics }: { diagnostics: unknown }) {
  const route = airplayRoute(diagnostics);
  return (
    <Stack gap="sm">
      <Text variant="label" muted>
        AIRPLAY
      </Text>
      <Facts
        rows={[
          ['Protocol', route.protocol],
          ['Clock', <Badge key="clock" label={route.clock} tone={route.tone} />],
        ]}
      />
    </Stack>
  );
}

function Endpoint({ endpoint }: { endpoint: AdminEndpoint }) {
  const colors = useTreeColors();
  const { facts, capabilities: caps, health } = endpoint;
  const quirks = Object.entries(endpoint.quirks).filter(([, value]) => value !== null);
  return (
    <Card>
      <View style={styles.header}>
        <Text variant="heading">
          {endpoint.provider} · {endpoint.id}
        </Text>
        <Stack horizontal gap="xs">
          <Badge
            label={endpoint.available ? 'available' : 'unavailable'}
            tone={endpoint.available ? 'good' : 'danger'}
          />
          <Badge label={health.level} tone={healthTone(health.level)} />
          {endpoint.app ? <Badge label="app" /> : null}
        </Stack>
      </View>
      <View style={styles.columns}>
        <Stack gap="sm" style={styles.column}>
          <Text variant="label" muted>
            IDENTITY
          </Text>
          <Facts
            rows={[
              ['Name', endpoint.name],
              ['Model', dash(endpoint.model)],
              ['Manufacturer', dash(endpoint.manufacturer)],
              ['Software', dash(endpoint.software_version)],
              ['Address', dash(facts.address)],
              ['MAC', dash(facts.mac)],
              ['Serial', dash(facts.serial)],
              ['UUID', dash(facts.uuid)],
              ['Features', dash(facts.features)],
              ['Status flags', dash(facts.status_flags)],
              ['Follows natively', dash(endpoint.native_leader)],
            ]}
          />
        </Stack>
        <Stack gap="sm" style={styles.column}>
          <Text variant="label" muted>
            CAPABILITIES
          </Text>
          <Facts
            rows={[
              ['Formats', caps.formats.map(formatText).join('\n') || '–'],
              ['Lead', `${caps.lead_ms} ms`],
              ['Plays in sync', caps.sync_group ? 'yes' : 'no'],
              ['Group size', dash(caps.max_group_size)],
              ['Volume / mute', `${caps.volume ? 'yes' : 'no'} / ${caps.mute ? 'yes' : 'no'}`],
              ['Pairing', caps.pairing],
              [
                'Native group',
                caps.native_group
                  ? `${caps.native_group.family} ${caps.native_group.version}`
                  : '–',
              ],
              ['Withdrawn', caps.withdrawn.join(', ') || '–'],
              ['Volume', `${dash(endpoint.volume)}${endpoint.muted ? ' (muted)' : ''}`],
            ]}
          />
        </Stack>
      </View>
      {endpoint.provider === 'airplay' ? (
        <AirplayRouteRows diagnostics={endpoint.diagnostics} />
      ) : null}
      <Stack gap="sm">
        <Text variant="label" muted>
          QUIRKS
        </Text>
        {quirks.length === 0 ? (
          <Text muted>None.</Text>
        ) : (
          <Facts
            rows={quirks.map(([name, value]) => [
              name,
              `${JSON.stringify(value)}  ·  ${source(endpoint.quirk_sources[name])}`,
            ])}
          />
        )}
      </Stack>
      <Stack gap="sm">
        <Text variant="label" muted>
          HEALTH
        </Text>
        <Text>
          {Object.entries(health.counts)
            .map(([event, count]) => `${event} ${count}`)
            .join(' · ') || 'Nothing recorded yet.'}
        </Text>
        {health.recent.slice(0, 10).map((event) => (
          <Text key={`${event.event}-${event.ago_s}`} variant="mono" muted>
            {`${event.ago_s.toFixed(1)} s ago  ${event.event}`}
          </Text>
        ))}
      </Stack>
      {endpoint.diagnostics === null ? null : (
        <Disclosure inline title={`${endpoint.provider} state`}>
          <JsonTree value={json(endpoint.diagnostics)} colors={colors} />
        </Disclosure>
      )}
    </Card>
  );
}

function Plan({ group, names }: { group: AdminGroup; names: Map<string, string> }) {
  const name = (id: string) => names.get(id) ?? id;
  const plan = group.plan;
  return (
    <Card title={`Group led by ${name(group.leader)}`}>
      {plan ? (
        <Facts
          rows={[
            ['Strategy', plan.strategy],
            ['Format', `${plan.sample_rate / 1000} kHz, ${plan.channels} ch`],
            ['Lead', `${plan.lead_ms} ms`],
            [
              'Carriers',
              plan.carriers
                .map(
                  (c) =>
                    `${name(c.device)} (${c.endpoint}) ${formatText(c.format)}${c.native ? ', native' : ''}`,
                )
                .join('\n'),
            ],
            ['Left out', plan.dropped.map((d) => d.message).join('\n') || 'nobody'],
          ]}
        />
      ) : (
        <Text muted>Not playing. Members: {group.members.map(name).join(', ') || 'none'}.</Text>
      )}
    </Card>
  );
}

export default function PlayerScreen() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const devices = useDevices();
  const players = usePlayers();
  const groups = useGroups();
  const colors = useTreeColors();
  const error = devices.error ?? players.error ?? groups.error;
  const rows = playerRows(devices.data ?? [], players.data ?? [], groups.data ?? []);
  const row = rows.find((r) => r.id === id);
  const device = devices.data?.find((d) => d.id === id);
  const group = groups.data?.find((g) => g.leader === id || g.members.includes(id));
  const names = new Map((devices.data ?? []).map((d) => [d.id, d.name]));

  return (
    <Page title={device?.name ?? id} back={{ href: '/players', label: 'Players' }}>
      <InlineError message={error ? describeError(error) : null} />
      {devices.isSuccess && !device ? <Text muted>The server knows no player {id}.</Text> : null}
      {row ? <PlayerCard row={row} rows={rows} linked={false} /> : null}
      {device ? <PlayerSettings device={device} /> : null}
      {group ? <Plan group={group} names={names} /> : null}
      {device?.members.map((endpoint) => (
        <Endpoint key={endpoint.id} endpoint={endpoint} />
      ))}
      {device ? (
        <Disclosure title="Raw" hint="What the server reports">
          <JsonTree
            value={json({ device, player: row?.player ?? null, group: group ?? null })}
            colors={colors}
          />
        </Disclosure>
      ) : null}
    </Page>
  );
}

const styles = StyleSheet.create((theme) => ({
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    flexWrap: 'wrap',
    gap: theme.spacing.sm,
  },
  columns: { flexDirection: { xs: 'column', lg: 'row' }, gap: theme.spacing.lg },
  column: { flexGrow: 1, flexBasis: { xs: 'auto', lg: 0 } },
}));
