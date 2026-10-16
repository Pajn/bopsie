import type { AdminServer } from '@bopsie/client/api/schemas/admin';
import { useLocalSearchParams } from 'expo-router';
import { useState } from 'react';
import { useConnection } from '@/connection/ConnectionProvider';
import { useServer } from '@/data/admin';
import JsonTree from '@/debug/JsonTree';
import { useTreeColors } from '@/debug/treeColors';
import { SpotifyCard } from '@/features/spotify/SpotifyCard';
import { describeError } from '@/lib/errors';
import { duration } from '@/lib/time';
import { Badge, Button, Card, Facts, InlineError, Page, Stack, Text } from '@/ui/primitives';

const yesNo = (value: boolean) => (value ? 'yes' : 'no');

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
      <Card title="Server">
        <Facts
          rows={[
            ['Version', view?.version ?? info?.server_version ?? '–'],
            ['Up for', view ? duration(view.uptime_s) : '–'],
            ['Server id', info?.server_id ?? '–'],
            ['Schema', String(info?.schema_version ?? '–')],
            ['AirPlay', view ? yesNo(view.providers.airplay) : '–'],
            [
              'PTP daemon',
              view?.providers.airplay_ptp_daemon
                ? `pid ${view.providers.airplay_ptp_daemon}`
                : 'not running',
            ],
            ['LinkPlay', view ? yesNo(view.providers.linkplay) : '–'],
            ['Sendspin', view ? yesNo(view.providers.sendspin) : '–'],
            ['ffmpeg', view ? yesNo(view.ffmpeg) : '–'],
          ]}
        />
      </Card>
      <Card title="Library">
        {view ? (
          <Facts
            rows={[
              ['Sources', view.library.providers.join(', ') || 'none'],
              ['Artists', String(view.library.artists)],
              ['Albums', String(view.library.albums)],
              ['Tracks', String(view.library.tracks)],
              ['Playlists', String(view.library.playlists)],
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
          <Button title="Sync" onPress={() => startSync(false)} />
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
      <SpotifyCard
        outcome={spotifyError ? `Logging in to Spotify failed: ${spotifyError}` : null}
      />
      {view ? (
        <Card title="Config">
          <JsonTree value={JSON.parse(JSON.stringify(view.config))} colors={colors} />
        </Card>
      ) : null}
    </Page>
  );
}

type Metadata = NonNullable<AdminServer['library']['metadata']>;

/** How far the library's albums have been looked up on MusicBrainz, and how the last run went. */
function musicBrainzRow(metadata: Metadata): [string, string] {
  if (!metadata.enabled) return ['MusicBrainz', 'off'];
  const progress = `${metadata.checked} of ${metadata.albums} albums looked up, ${metadata.identified} identified, ${metadata.suggested} to decide`;
  const last = metadata.last_run;
  const run = metadata.running
    ? 'looking up now'
    : last
      ? `last run ${at(last.finished)}: ${last.matched} matched, ${last.suggested} suggested, ${last.unmatched} unmatched, ${last.failed} failed`
      : 'no run since the server started';
  return ['MusicBrainz', `${progress}\n${run}`];
}
