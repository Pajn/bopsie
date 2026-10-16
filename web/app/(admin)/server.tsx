import type { AdminServer } from '@bopsie/client/api/schemas/admin';
import { Link, useLocalSearchParams } from 'expo-router';
import { type ReactNode, useState } from 'react';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { useServer } from '@/data/admin';
import JsonTree from '@/debug/JsonTree';
import { useTreeColors } from '@/debug/treeColors';
import { SpotifyCard } from '@/features/spotify/SpotifyCard';
import { describeError } from '@/lib/errors';
import { uptime } from '@/lib/time';
import { Disclosure } from '@/ui/controls';
import { Badge, Button, Card, Facts, InlineError, Page, Stack, Text } from '@/ui/primitives';

const at = (unixSeconds: number) => new Date(unixSeconds * 1000).toLocaleString();

export default function ServerScreen() {
  const { connection } = useConnection();
  const colors = useTreeColors();
  const server = useServer();
  const [error, setError] = useState<string | null>(null);
  const info = connection.serverInfo;
  const view = server.data;
  const sync = view?.library.last_sync;
  const metadata = view?.library.metadata;
  // A Spotify login comes back here, saying how it went.
  const { spotify_error: spotifyError } = useLocalSearchParams<{ spotify_error?: string }>();

  const startSync = (force: boolean) => {
    setError(null);
    connection.command('music/sync', { force }).catch((e: unknown) => setError(describeError(e)));
  };

  return (
    <Page title={info?.name ?? 'Server'}>
      <InlineError message={server.error ? describeError(server.error) : null} />
      <Card title="Library">
        {view ? (
          <View style={styles.stats}>
            <Stat label="Artists" value={view.library.artists} />
            <Stat label="Albums" value={view.library.albums} />
            <Stat label="Tracks" value={view.library.tracks} />
            <Stat label="Playlists" value={view.library.playlists} />
          </View>
        ) : null}
        {view ? (
          <Facts
            rows={[
              ['Sources', view.library.providers.join(', ') || 'none'],
              [
                'Last sync',
                sync
                  ? `${at(sync.finished)}, ${sync.finished - sync.started} s: ${sync.added} added, ${sync.updated} updated, ${sync.deleted} deleted`
                  : 'none since the server started',
              ],
              [
                'Failed sources',
                sync?.failed.map((f) => `${f.provider}: ${f.error}`).join('\n') || 'none',
              ],
              ...(metadata ? [musicBrainzRow(metadata)] : []),
            ]}
          />
        ) : null}
        <Stack horizontal gap="sm">
          {view?.library.syncing ? <Badge label="syncing" tone="accent" /> : null}
          <Button title="Sync" icon="refresh-cw" onPress={() => startSync(false)} />
          <Button
            title="Sync, dropping missing sources"
            variant="secondary"
            onPress={() => startSync(true)}
          />
        </Stack>
        <Text variant="caption" muted>
          A sync keeps the tracks of a source that lists nothing, such as an unmounted disk. The
          second button removes them.
        </Text>
        <InlineError message={error} />
      </Card>
      <Card title="Server">
        <Facts
          rows={[
            ['Version', view?.version ?? info?.server_version ?? '–'],
            ['Up for', view ? uptime(view.uptime_s) : '–'],
            ['Server id', info?.server_id ?? '–'],
            ['Schema', String(info?.schema_version ?? '–')],
            ['AirPlay', <OnOff key="airplay" on={view?.providers.airplay} />],
            [
              'PTP daemon',
              view?.providers.airplay_ptp_daemon
                ? `pid ${view.providers.airplay_ptp_daemon}`
                : 'not running',
            ],
            ['LinkPlay', <OnOff key="linkplay" on={view?.providers.linkplay} />],
            ['Sendspin', <OnOff key="sendspin" on={view?.providers.sendspin} />],
            ['ffmpeg', <OnOff key="ffmpeg" on={view?.ffmpeg} yes="found" no="not found" />],
          ]}
        />
      </Card>
      <SpotifyCard
        outcome={spotifyError ? `Logging in to Spotify failed: ${spotifyError}` : null}
      />
      {view ? (
        <Disclosure title="Config" hint="As the server read it">
          <JsonTree value={JSON.parse(JSON.stringify(view.config))} colors={colors} />
        </Disclosure>
      ) : null}
    </Page>
  );
}

/** Whether something is on, or found, as a badge. */
function OnOff({
  on,
  yes = 'on',
  no = 'off',
}: {
  on: boolean | undefined;
  yes?: string;
  no?: string;
}) {
  if (on === undefined) return <Text muted>–</Text>;
  return <Badge label={on ? yes : no} tone={on ? 'good' : 'muted'} />;
}

/** A count, large, over what it counts. */
function Stat({ label, value }: { label: string; value: number }) {
  return (
    <View style={styles.stat}>
      <Text style={styles.statValue}>{value.toLocaleString()}</Text>
      <Text variant="caption" muted>
        {label}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  stats: { flexDirection: 'row', flexWrap: 'wrap', gap: theme.spacing.sm },
  stat: {
    flexGrow: 1,
    flexBasis: { xs: '40%', md: 0 },
    padding: theme.spacing.md,
    borderRadius: theme.radius.md,
    backgroundColor: theme.colors.surfaceVariant,
  },
  statValue: { ...theme.typography.title, color: theme.colors.text },
  link: { color: theme.colors.accent, fontWeight: '600' },
}));

type Metadata = NonNullable<AdminServer['library']['metadata']>;

/** How far the library's albums have been looked up on MusicBrainz, and how the last run went. */
function musicBrainzRow(metadata: Metadata): [string, ReactNode] {
  if (!metadata.enabled) return ['MusicBrainz', 'off'];
  const last = metadata.last_run;
  const run = metadata.running
    ? 'looking up now'
    : last
      ? `last run ${at(last.finished)}: ${last.matched} matched, ${last.suggested} suggested, ${last.unmatched} unmatched, ${last.failed} failed`
      : 'no run since the server started';
  return [
    'MusicBrainz',
    <View key="musicbrainz">
      <Text>
        {metadata.checked} of {metadata.albums} albums looked up, {metadata.identified} identified
        {metadata.suggested > 0 ? ', ' : ''}
        {metadata.suggested > 0 ? (
          <Link href="/library?tab=suggestions">
            {/* Styled inside: the link merges its child's style, which loses unistyles' classes. */}
            <Text style={styles.link}>{metadata.suggested} to decide</Text>
          </Link>
        ) : null}
      </Text>
      <Text muted>{run}</Text>
    </View>,
  ];
}
