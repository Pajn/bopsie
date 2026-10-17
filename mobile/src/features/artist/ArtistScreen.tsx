import {
  artistAlbums,
  artistAudiobooks,
  artistTracks,
  getArtist,
} from '@bopsie/client/api/endpoints/library';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { LegendList } from '@legendapp/list/react-native';
import { useQuery } from '@tanstack/react-query';
import { Stack } from 'expo-router';
import { useMemo, useState } from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { ItemHeader } from '@/features/library/ItemHeader';
import { MoreButton } from '@/features/library/menu';
import { AlbumRow, BookRow, TrackRow } from '@/features/library/rows';
import { useOpenItemMenu } from '@/features/library/useOpenItemMenu';
import { usePlaybackActions } from '@/features/player/usePlayback';
import { describeError } from '@/lib/errors';
import { InlineError, Text } from '@/ui/primitives';
import { SkeletonRows } from '@/ui/Skeleton';
import { type ArtistRow, artistRows } from './sections';

export function ArtistScreen({
  id,
  provider,
  name,
}: {
  id: string;
  provider: string;
  name?: string;
}) {
  const { connection, account } = useConnection();
  const playback = usePlaybackActions();
  const openMenu = useOpenItemMenu();
  const [expanded, setExpanded] = useState(false);
  const ref = { itemId: id, provider };
  const artist = useQuery({
    queryKey: queryKeys.item(account.id, 'artist', id, provider),
    queryFn: () => getArtist(connection, ref),
  });
  const albums = useQuery({
    queryKey: [...queryKeys.item(account.id, 'artist', id, provider), 'albums'],
    queryFn: () => artistAlbums(connection, ref),
  });
  const tracks = useQuery({
    queryKey: [...queryKeys.item(account.id, 'artist', id, provider), 'tracks'],
    queryFn: () => artistTracks(connection, ref),
  });
  // An author is an artist of the source that serves their books; elsewhere there are none.
  const books = useQuery({
    queryKey: [...queryKeys.item(account.id, 'artist', id, provider), 'audiobooks'],
    queryFn: () => artistAudiobooks(connection, ref),
  });
  const error = artist.error ?? albums.error ?? tracks.error ?? books.error;
  const loading = albums.isPending || tracks.isPending || books.isPending;
  const rows = useMemo(
    () =>
      artistRows(
        albums.data ?? [],
        tracks.data ?? [],
        { popular: provider !== 'library', expanded },
        books.data ?? [],
      ),
    [albums.data, tracks.data, books.data, provider, expanded],
  );
  const trackUris = useMemo(() => (tracks.data ?? []).map((track) => track.uri), [tracks.data]);

  const renderRow = (row: ArtistRow) => {
    switch (row.type) {
      case 'heading':
        return (
          <Text variant="label" muted style={styles.heading}>
            {row.title}
          </Text>
        );
      case 'track':
        // A track plays on through the artist's tracks after it.
        return (
          <TrackRow
            track={row.track}
            showArtwork
            onPress={() => playback.play(trackUris, row.track.uri)}
          />
        );
      case 'album':
        return <AlbumRow album={row.album} />;
      case 'book':
        return <BookRow book={row.book} />;
      case 'more':
        return (
          <Pressable
            onPress={() => setExpanded(true)}
            style={({ pressed }) => [styles.more, pressed && styles.pressed]}
            accessibilityRole="button"
            testID="artist-tracks-more"
          >
            <Text variant="label" style={styles.moreText}>
              {`Show ${row.hidden} more`}
            </Text>
          </Pressable>
        );
    }
  };

  return (
    <View style={styles.screen}>
      <Stack.Screen
        options={{
          title: artist.data?.name ?? name ?? '',
          headerRight: () => {
            const loaded = artist.data;
            return loaded ? (
              <MoreButton onPress={() => openMenu('artist', loaded)} testID="item-more" />
            ) : null;
          },
        }}
      />
      <LegendList
        data={rows}
        keyExtractor={(row) => row.key}
        getItemType={(row) => row.type}
        renderItem={({ item }) => renderRow(item)}
        estimatedItemSize={64}
        ListHeaderComponent={
          <>
            <ItemHeader
              item={artist.data ?? null}
              title={artist.data?.name ?? name ?? ''}
              onPlay={() => (artist.data ? playback.play(artist.data.uri) : undefined)}
              onShuffle={() => (artist.data ? playback.shufflePlay(artist.data.uri) : undefined)}
              round
              placeholder="user"
            />
            {error ? <InlineError message={describeError(error)} /> : null}
          </>
        }
        ListEmptyComponent={
          loading ? (
            <SkeletonRows count={6} />
          ) : error ? null : (
            <Text muted style={styles.empty}>
              No albums or tracks.
            </Text>
          )
        }
        contentContainerStyle={styles.content}
        testID="artist-list"
      />
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  screen: { flex: 1, backgroundColor: theme.colors.background },
  content: { paddingHorizontal: theme.spacing.lg, paddingBottom: theme.spacing.xxl },
  heading: { marginTop: theme.spacing.lg, marginBottom: theme.spacing.xs },
  more: { paddingVertical: theme.spacing.md },
  moreText: { color: theme.colors.accent },
  pressed: { opacity: 0.6 },
  empty: { marginTop: theme.spacing.xl, textAlign: 'center' },
}));
