import { refreshMetadata, separateAlbum } from '@bopsie/client/api/endpoints/admin';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQueryClient } from '@tanstack/react-query';
import { useRouter } from 'expo-router';
import { useState } from 'react';
import { Linking } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { describeError } from '@/lib/errors';
import { Badge, Button, Card, Facts, InlineError, Stack, Text } from '@/ui/primitives';
import type { Item } from './ItemRow';

/** MusicBrainz's page for an id of `kind`, if it has one. */
function idUrl(kind: string, value: string): string | null {
  const entity: Record<string, string> = {
    musicbrainz_albumid: 'release',
    musicbrainz_releasegroupid: 'release-group',
    musicbrainz_recordingid: 'recording',
    musicbrainz_trackid: 'track',
    musicbrainz_artistid: 'artist',
    isrc: 'isrc',
  };
  return entity[kind] ? `https://musicbrainz.org/${entity[kind]}/${value}` : null;
}

/** What an external id is, as a person reads it; one not known here keeps its own name. */
const ID_NAMES: Record<string, string> = {
  musicbrainz_albumid: 'MusicBrainz release',
  musicbrainz_releasegroupid: 'MusicBrainz release group',
  musicbrainz_recordingid: 'MusicBrainz recording',
  musicbrainz_trackid: 'MusicBrainz track',
  musicbrainz_artistid: 'MusicBrainz artist',
  isrc: 'ISRC',
  barcode: 'Barcode',
};

/** A provider's own page for what it holds, if it has one people can open. */
function mappingUrl(domain: string, mediaType: string, id: string): string | null {
  return domain === 'spotify' ? `https://open.spotify.com/${mediaType}/${id}` : null;
}

function Linked({ text, url }: { text: string; url: string | null }) {
  return url ? (
    <Text style={styles.link} onPress={() => void Linking.openURL(url)}>
      {text}
    </Text>
  ) : (
    <Text>{text}</Text>
  );
}

/**
 * The providers an item comes from, the one it plays from first at the top. An album merged
 * from several can have one taken back out into an album of its own, which then stays apart.
 */
export function Sources({ item }: { item: Item }) {
  const { account, connection } = useConnection();
  const queryClient = useQueryClient();
  const router = useRouter();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mappings = item.provider_mappings;
  const instances = new Set(mappings.map((mapping) => mapping.provider_instance));
  const separable = item.media_type === 'album' && instances.size > 1;
  const [refreshed, setRefreshed] = useState(false);
  // Each kind once, with all its values: a track can have several ISRCs.
  const byKind = new Map<string, string[]>();
  for (const [kind, value] of item.external_ids) {
    byKind.set(kind, [...(byKind.get(kind) ?? []), value]);
  }
  const ids = [...byKind];

  const refresh = async () => {
    setError(null);
    setBusy('refresh');
    try {
      await refreshMetadata(connection, item.item_id);
      setRefreshed(true);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(null);
    }
  };

  const separate = async (instance: string) => {
    setError(null);
    setBusy(instance);
    try {
      const { item_id } = await separateAlbum(connection, item.item_id, instance);
      await queryClient.invalidateQueries({ queryKey: queryKeys.library(account.id) });
      if (item_id) router.push(`/library/album/library/${encodeURIComponent(item_id)}`);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card title="Sources">
      {separable ? (
        <Text variant="caption" muted>
          Merged from several sources. Separate takes one back out, with the tracks only it holds,
          into an album of its own that is not merged again, unless reconsidered under Suggestions.
        </Text>
      ) : null}
      <InlineError message={error} />
      {mappings.map((mapping, index) => (
        <Stack key={`${mapping.provider_instance}/${mapping.item_id}`} gap="xs">
          <Stack horizontal gap="xs">
            <Text variant="label">{mapping.provider_domain}</Text>
            {index === 0 && mappings.length > 1 ? (
              <Badge label="plays first" tone="accent" />
            ) : null}
          </Stack>
          <Facts
            rows={[
              ['Provider', mapping.provider_instance],
              [
                'Id',
                <Linked
                  key="id"
                  text={mapping.item_id}
                  url={mappingUrl(mapping.provider_domain, item.media_type, mapping.item_id)}
                />,
              ],
            ]}
          />
          {separable ? (
            <Button
              title="Separate"
              variant="secondary"
              loading={busy === mapping.provider_instance}
              disabled={busy !== null}
              onPress={() => void separate(mapping.provider_instance)}
            />
          ) : null}
        </Stack>
      ))}
      {ids.length ? (
        <Facts
          rows={ids.map(([kind, values]) => [
            ID_NAMES[kind] ?? kind,
            <Stack key={kind} gap="xs">
              {values.map((value) => (
                <Linked key={value} text={value} url={idUrl(kind, value)} />
              ))}
            </Stack>,
          ])}
        />
      ) : null}
      {item.media_type === 'album' ? (
        <Stack gap="xs">
          <Button
            title="Refresh metadata"
            variant="secondary"
            loading={busy === 'refresh'}
            disabled={busy !== null}
            onPress={() => void refresh()}
          />
          <Text variant="caption" muted>
            {refreshed
              ? 'Being looked up on MusicBrainz now; it shows here once found.'
              : 'Looks it up on MusicBrainz again now, for its ids, art and genres.'}
          </Text>
        </Stack>
      ) : null}
    </Card>
  );
}

const styles = StyleSheet.create((theme) => ({
  link: { color: theme.colors.accent },
}));
