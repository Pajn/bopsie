import {
  decideRelease,
  decideSuggestion,
  librarySuggestions,
  releaseSuggestions,
} from '@bopsie/client/api/endpoints/admin';
import type { LibrarySuggestion } from '@bopsie/client/api/schemas/admin';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Linking, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { describeError } from '@/lib/errors';
import { Button, Card, InlineError, Stack, Text } from '@/ui/primitives';
import { ItemRow } from './ItemRow';

/** Where each side comes from, as its sources name it. */
function sourcesOf(item: LibrarySuggestion['a']): string {
  return item.provider_mappings.map((mapping) => mapping.provider_domain).join(', ');
}

/**
 * Albums from different providers that may be the same release, such as a deluxe edition of
 * an album: side by side, to merge or keep apart. A decision holds at every later sync.
 */
export function Suggestions() {
  const { account, connection } = useConnection();
  const queryClient = useQueryClient();
  const key = queryKeys.admin(account.id, 'suggestions');
  const suggestions = useQuery({ queryKey: key, queryFn: () => librarySuggestions(connection) });
  const releasesKey = queryKeys.admin(account.id, 'release-suggestions');
  const releases = useQuery({
    queryKey: releasesKey,
    queryFn: () => releaseSuggestions(connection),
  });
  const [busy, setBusy] = useState<number | string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const decide = async (id: number, decision: 'merge' | 'apart') => {
    setError(null);
    setBusy(id);
    try {
      await decideSuggestion(connection, id, decision);
      await queryClient.invalidateQueries({ queryKey: key });
      await queryClient.invalidateQueries({ queryKey: queryKeys.library(account.id) });
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(null);
    }
  };

  const decideOnRelease = async (albumId: string, decision: 'accept' | 'reject') => {
    setError(null);
    setBusy(albumId);
    try {
      await decideRelease(connection, albumId, decision);
      await queryClient.invalidateQueries({ queryKey: releasesKey });
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(null);
    }
  };

  const failed = suggestions.error ?? releases.error;
  const error_ = error ?? (failed ? describeError(failed) : null);
  return (
    <Stack gap="md">
      <Text variant="caption" muted>
        Albums from different sources that may be the same release. The same release is merged when
        a sync finds it; these are close but not certain, such as another edition.
      </Text>
      <InlineError message={error_} />
      {suggestions.isSuccess &&
      suggestions.data.length === 0 &&
      releases.isSuccess &&
      releases.data.length === 0 ? (
        <Card>
          <Text muted>Nothing to decide.</Text>
        </Card>
      ) : null}
      {suggestions.data?.map((suggestion) => (
        <Card key={suggestion.id}>
          <Text variant="caption" muted>
            {suggestion.reason}
          </Text>
          {[suggestion.a, suggestion.b].map((side) => (
            <View key={side.uri} style={styles.side}>
              <ItemRow item={side} />
              <Text variant="caption" muted>
                {sourcesOf(side)}
              </Text>
            </View>
          ))}
          <Stack horizontal gap="xs">
            <Button
              title="Merge"
              loading={busy === suggestion.id}
              onPress={() => void decide(suggestion.id, 'merge')}
            />
            <Button
              title="Keep apart"
              variant="secondary"
              disabled={busy === suggestion.id}
              onPress={() => void decide(suggestion.id, 'apart')}
            />
          </Stack>
        </Card>
      ))}
      {releases.data?.length ? (
        <Text variant="caption" muted>
          Albums that look like a release on MusicBrainz, though not closely enough to be sure. A
          release used gives the album its ids, and ties it to the same album elsewhere.
        </Text>
      ) : null}
      {releases.data?.map((suggestion) => (
        <Card key={suggestion.album.uri}>
          <ItemRow item={suggestion.album} />
          <Text>Looks like {suggestion.title}</Text>
          <Text
            variant="caption"
            style={styles.link}
            onPress={() => void Linking.openURL(suggestion.url)}
          >
            {suggestion.url}
          </Text>
          <Stack horizontal gap="xs">
            <Button
              title="Use this release"
              loading={busy === suggestion.album.item_id}
              onPress={() => void decideOnRelease(suggestion.album.item_id, 'accept')}
            />
            <Button
              title="Not this"
              variant="secondary"
              disabled={busy === suggestion.album.item_id}
              onPress={() => void decideOnRelease(suggestion.album.item_id, 'reject')}
            />
          </Stack>
        </Card>
      ))}
    </Stack>
  );
}

const styles = StyleSheet.create((theme) => ({
  side: { gap: theme.spacing.xs },
  link: { color: theme.colors.accent },
}));
