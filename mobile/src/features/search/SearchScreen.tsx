import { search } from '@bopsie/client/api/endpoints/library';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { ActivityIndicator, Pressable, ScrollView, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { AlbumRow, ArtistRow, PlaylistRow, TrackRow } from '@/features/library/rows';
import { usePlaybackActions } from '@/features/player/usePlayback';
import { describeError } from '@/lib/errors';
import { useDebounced } from '@/lib/useDebounced';
import { InlineError, Screen, Stack, Text, TextField } from '@/ui/primitives';
import { EVERYWHERE, type SearchScope, searchScopes } from './scope';
import { useProviders } from './useProviders';

export function SearchScreen() {
  const { connection, account } = useConnection();
  const playback = usePlaybackActions();
  const { theme } = useUnistyles();
  const [text, setText] = useState('');
  const query = useDebounced(text.trim(), 300);
  const scopes = searchScopes(useProviders().data ?? []);
  // Kept while the tab is, so the next search looks where the last one did.
  const [scopeKey, setScopeKey] = useState(EVERYWHERE.key);
  const scope: SearchScope = scopes.find((s) => s.key === scopeKey) ?? EVERYWHERE;
  const results = useQuery({
    queryKey: queryKeys.search(account.id, query, scope.key),
    queryFn: () => search(connection, query, 25, scope.providers),
    enabled: query.length > 0,
  });
  const data = results.data;

  return (
    <Screen>
      <Text variant="title" style={styles.title}>
        Search
      </Text>
      <TextField
        label="Artists, albums, tracks and playlists"
        value={text}
        onChangeText={setText}
        autoCapitalize="none"
        autoCorrect={false}
        returnKeyType="search"
        testID="search-input"
      />
      {scopes.length > 0 ? (
        <View style={styles.scopes} accessibilityRole="tablist">
          {scopes.map((option) => {
            const selected = option.key === scope.key;
            return (
              <Pressable
                key={option.key}
                onPress={() => setScopeKey(option.key)}
                style={[styles.scope, selected && styles.scopeSelected]}
                accessibilityRole="tab"
                accessibilityState={{ selected }}
                testID={`search-scope-${option.key}`}
              >
                <Text variant="caption" style={selected ? styles.scopeSelectedText : undefined}>
                  {option.label}
                </Text>
              </Pressable>
            );
          })}
        </View>
      ) : null}
      <ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={styles.results}>
        {results.isFetching && !data ? <ActivityIndicator color={theme.colors.accent} /> : null}
        {results.isError ? <InlineError message={describeError(results.error)} /> : null}
        {data ? (
          <Stack gap="lg">
            {data.artists.length > 0 ? (
              <Section title="Artists">
                {data.artists.map((artist) => (
                  <ArtistRow key={artist.uri} artist={artist} />
                ))}
              </Section>
            ) : null}
            {data.albums.length > 0 ? (
              <Section title="Albums">
                {data.albums.map((album) => (
                  <AlbumRow key={album.uri} album={album} />
                ))}
              </Section>
            ) : null}
            {data.tracks.length > 0 ? (
              <Section title="Tracks">
                {data.tracks.map((track) => (
                  <TrackRow
                    key={track.uri}
                    track={track}
                    showArtwork
                    onPress={() => playback.play(track.uri)}
                  />
                ))}
              </Section>
            ) : null}
            {data.playlists.length > 0 ? (
              <Section title="Playlists">
                {data.playlists.map((playlist) => (
                  <PlaylistRow key={playlist.uri} playlist={playlist} />
                ))}
              </Section>
            ) : null}
          </Stack>
        ) : null}
      </ScrollView>
    </Screen>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <View>
      <Text variant="label" muted>
        {title}
      </Text>
      {children}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  title: { marginVertical: theme.spacing.md },
  scopes: { flexDirection: 'row', gap: theme.spacing.sm, marginTop: theme.spacing.md },
  scope: {
    paddingHorizontal: theme.spacing.md,
    paddingVertical: theme.spacing.xs,
    borderRadius: 999,
    borderWidth: 1,
    borderColor: theme.colors.border,
  },
  scopeSelected: { backgroundColor: theme.colors.accent, borderColor: theme.colors.accent },
  scopeSelectedText: { color: theme.colors.onAccent },
  results: { paddingVertical: theme.spacing.lg },
}));
