import { search } from '@bopsie/client/api/endpoints/library';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { ActivityIndicator, ScrollView, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { AlbumRow, ArtistRow, PlaylistRow, TrackRow } from '@/features/library/rows';
import { usePlaybackActions } from '@/features/player/usePlayback';
import { describeError } from '@/lib/errors';
import { InlineError, Screen, Stack, Text, TextField } from '@/ui/primitives';

/** Waits for typing to pause before searching, so each keystroke is not a request. */
function useDebounced(value: string, ms: number): string {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(timer);
  }, [value, ms]);
  return debounced;
}

export function SearchScreen() {
  const { connection, account } = useConnection();
  const playback = usePlaybackActions();
  const { theme } = useUnistyles();
  const [text, setText] = useState('');
  const query = useDebounced(text.trim(), 300);
  const results = useQuery({
    queryKey: queryKeys.search(account.id, query),
    queryFn: () => search(connection, query),
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
  results: { paddingVertical: theme.spacing.lg },
}));
