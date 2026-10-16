import {
  decideRelease,
  decideSuggestion,
  librarySuggestions,
  reconsider,
  reconsiderRelease,
  releaseSuggestions,
} from '@bopsie/client/api/endpoints/admin';
import type { LibrarySuggestion, ReleaseSuggestion } from '@bopsie/client/api/schemas/admin';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Linking, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { describeError } from '@/lib/errors';
import { Disclosure, IconButton } from '@/ui/controls';
import { Button, Card, InlineError, Stack, Text } from '@/ui/primitives';
import { ItemRow } from './ItemRow';
import { releaseFacts } from './releases';

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
  const merges = suggestions.data ?? [];
  const matches = releases.data ?? [];
  return (
    <Stack gap="md">
      <InlineError message={error_} />
      {suggestions.isSuccess &&
      merges.length === 0 &&
      releases.isSuccess &&
      matches.length === 0 ? (
        <Card>
          <Text muted>Nothing to decide.</Text>
        </Card>
      ) : null}
      {merges.length > 0 ? (
        <View style={styles.section}>
          <Text variant="label" muted>
            THE SAME RELEASE? · {merges.length}
          </Text>
          <Text variant="caption" muted>
            Albums from different sources that may be the same release. The same release is merged
            when a sync finds it; these are close but not certain, such as another edition.
          </Text>
        </View>
      ) : null}
      {merges.map((suggestion) => (
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
      {matches.length > 0 ? (
        <View style={styles.section}>
          <Text variant="label" muted>
            MUSICBRAINZ MATCHES TO CHECK · {matches.length}
          </Text>
          <Text variant="caption" muted>
            Albums that look like a release on MusicBrainz, though not closely enough to be sure. A
            release used gives the album its ids, and ties it to the same album elsewhere.
          </Text>
        </View>
      ) : null}
      {matches.length > 0 ? (
        <Card style={styles.list}>
          {matches.map((suggestion, i) => (
            <View
              key={suggestion.album.uri}
              style={[styles.match, i < matches.length - 1 && styles.divided]}
            >
              <View style={styles.matchAlbum}>
                <ItemRow item={suggestion.album} divided={false} />
              </View>
              <View style={styles.matchRelease}>
                <Text variant="caption" muted>
                  Looks like
                </Text>
                <ReleaseSummary suggestion={suggestion} />
              </View>
              <View style={styles.matchActions}>
                <IconButton
                  icon="external-link"
                  label="Open the release on MusicBrainz"
                  variant="ghost"
                  size="sm"
                  onPress={() => void Linking.openURL(suggestion.url)}
                />
                <Button
                  title="Use"
                  loading={busy === suggestion.album.item_id}
                  disabled={busy !== null && busy !== suggestion.album.item_id}
                  onPress={() => void decideOnRelease(suggestion.album.item_id, 'accept')}
                />
                <Button
                  title="Not this"
                  variant="secondary"
                  disabled={busy !== null}
                  onPress={() => void decideOnRelease(suggestion.album.item_id, 'reject')}
                />
              </View>
            </View>
          ))}
        </Card>
      ) : null}
      <Decided />
    </Stack>
  );
}

/**
 * The release a match suggests, beside the album: its title where it reads differently, and its
 * artist, year, country, formats and tracks, what differs from the album marked. A suggestion
 * made before the server kept these says its title alone.
 */
function ReleaseSummary({ suggestion }: { suggestion: ReleaseSuggestion }) {
  if (!suggestion.release) return <Text numberOfLines={2}>{suggestion.title}</Text>;
  const { title, details } = releaseFacts(suggestion.album, suggestion.release);
  return (
    <View>
      <Text numberOfLines={1} style={title.differs && styles.differs}>
        {title.text}
      </Text>
      <Text variant="caption" muted numberOfLines={2}>
        {details.map((fact, i) => (
          // Each part is a text of its own, which takes the caption's look only when told.
          <Text key={fact.kind} variant="caption" muted style={fact.differs && styles.differs}>
            {i > 0 ? ' · ' : ''}
            {fact.text}
          </Text>
        ))}
      </Text>
    </View>
  );
}

const DECISIONS: Record<string, string> = {
  merge: 'Merged',
  apart: 'Kept apart',
  accept: 'Release used',
  reject: 'Release rejected',
};

/**
 * What was decided before: merges, albums kept apart (by hand or by Separate), and releases
 * used or rejected. Reconsider forgets a decision, and the two are compared, or the album
 * looked up, again at once.
 */
function Decided() {
  const { account, connection } = useConnection();
  const queryClient = useQueryClient();
  const albums = useQuery({
    queryKey: queryKeys.admin(account.id, 'suggestions-decided'),
    queryFn: async () =>
      (await librarySuggestions(connection, true)).filter((s) => s.decision !== null),
  });
  const releases = useQuery({
    queryKey: queryKeys.admin(account.id, 'release-suggestions-decided'),
    queryFn: async () =>
      (await releaseSuggestions(connection, true)).filter((s) => s.decision !== null),
  });
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const undo = async (key: string, action: () => Promise<void>) => {
    setError(null);
    setBusy(key);
    try {
      await action();
      await queryClient.invalidateQueries({ queryKey: ['admin', account.id] });
      await queryClient.invalidateQueries({ queryKey: queryKeys.library(account.id) });
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(null);
    }
  };

  const count = (albums.data?.length ?? 0) + (releases.data?.length ?? 0);
  if (count === 0) return null;
  return (
    <Disclosure title={`Decided · ${count}`} hint="Reconsider any">
      <Text variant="caption" muted>
        Reconsider forgets a decision: the albums are compared again at once, and merge if they are
        the same release, which undoes a Separate; an album is looked up again.
      </Text>
      <InlineError message={error} />
      {albums.data?.map((s) => (
        <View key={`a${s.id}`} style={styles.side}>
          <Text variant="label">
            {DECISIONS[s.decision ?? ''] ?? s.decision}: {s.reason}
          </Text>
          <ItemRow item={s.a} />
          <ItemRow item={s.b} />
          <Button
            title="Reconsider"
            variant="secondary"
            loading={busy === `a${s.id}`}
            disabled={busy !== null}
            onPress={() => void undo(`a${s.id}`, () => reconsider(connection, s.id))}
          />
        </View>
      ))}
      {releases.data?.map((s) => (
        <View key={`r${s.album.item_id}`} style={styles.side}>
          <Text variant="label">
            {DECISIONS[s.decision ?? ''] ?? s.decision}: {s.title}
          </Text>
          <ItemRow item={s.album} />
          <Button
            title="Reconsider"
            variant="secondary"
            loading={busy === `r${s.album.item_id}`}
            disabled={busy !== null}
            onPress={() =>
              void undo(`r${s.album.item_id}`, () => reconsiderRelease(connection, s.album.item_id))
            }
          />
        </View>
      ))}
    </Disclosure>
  );
}

const styles = StyleSheet.create((theme) => ({
  side: { gap: theme.spacing.xs },
  // What the release says otherwise than the album, which is what decides a match.
  differs: { color: theme.colors.warning, fontWeight: '600' },
  section: { gap: 2 },
  list: { padding: 0, gap: 0 },
  divided: { borderBottomWidth: 1, borderColor: theme.colors.border },
  // The album, what it looks like and the choice in a row; stacked on a phone.
  match: {
    flexDirection: { xs: 'column', md: 'row' },
    alignItems: { xs: 'stretch', md: 'center' },
    gap: { xs: theme.spacing.xs, md: theme.spacing.lg },
    paddingHorizontal: theme.spacing.lg,
    paddingVertical: theme.spacing.sm,
  },
  matchAlbum: { flexGrow: 1, flexBasis: { xs: 'auto', md: 0 }, minWidth: 0 },
  matchRelease: { flexGrow: 1.4, flexBasis: { xs: 'auto', md: 0 }, minWidth: 0 },
  matchActions: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'flex-end',
    gap: theme.spacing.xs,
  },
}));
