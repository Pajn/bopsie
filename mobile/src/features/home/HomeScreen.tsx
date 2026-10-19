import {
  inProgressItems,
  type LibraryKind,
  libraryItems,
} from '@bopsie/client/api/endpoints/library';
import type { Album, Artist, Playlist } from '@bopsie/client/api/schemas/media';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { type ReactNode, useState } from 'react';
import { Pressable, RefreshControl, ScrollView, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { AlbumRow, ArtistRow, BookRow, PlaylistRow } from '@/features/library/rows';
import { mark } from '@/lib/bench';
import { Icon } from '@/ui/Icon';
import { Screen, Text } from '@/ui/primitives';
import { greeting } from './greeting';

/** How wide a shelf's tiles are, and how many a shelf holds. */
const TILE = 140;
const SHELF = 12;

/** A shelf's items, from the library in an order, kept with the rest of the library's lists. */
function useShelf<T>(kind: Exclude<LibraryKind, 'tracks'>, orderBy: string, round = 0) {
  const { connection, account } = useConnection();
  return useQuery({
    queryKey: ['library', account.id, 'shelf', kind, orderBy, round],
    queryFn: () =>
      libraryItems(connection, kind, { limit: SHELF, offset: 0, orderBy }) as Promise<T[]>,
  });
}

/**
 * Where the app opens: what came into the library lately, the playlists played most, the
 * audiobooks listening continues in, the artists played most, and a handful of albums picked at
 * random to rediscover, each a shelf that scrolls sideways.
 */
export function HomeScreen() {
  const { theme } = useUnistyles();
  const queryClient = useQueryClient();
  const { connection, account } = useConnection();
  const [round, setRound] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const added = useShelf<Album>('albums', 'timestamp_added_desc');
  // The most played first; never played, the newest.
  const playlists = useShelf<Playlist>('playlists', 'play_count_desc');
  const listening = useQuery({
    queryKey: ['library', account.id, 'shelf', 'in_progress'],
    queryFn: () => inProgressItems(connection, SHELF),
  });
  const artists = useShelf<Artist>('artists', 'play_count_desc');
  const random = useShelf<Album>('albums', 'random', round);
  mark('home-render');
  if (added.data) mark('home-shelves');

  const refresh = async () => {
    setRefreshing(true);
    setRound((r) => r + 1);
    await queryClient.invalidateQueries({ queryKey: ['library', account.id, 'shelf'] });
    setRefreshing(false);
  };

  return (
    <Screen>
      <ScrollView
        contentContainerStyle={styles.content}
        onLayout={() => mark('home-mounted')}
        refreshControl={
          <RefreshControl
            refreshing={refreshing}
            onRefresh={refresh}
            tintColor={theme.colors.accent}
            colors={[theme.colors.accent]}
          />
        }
        testID="home"
      >
        <Text variant="title" style={styles.title}>
          {greeting(new Date().getHours())}
        </Text>
        <Shelf title="Recently added" loading={added.isPending}>
          {added.data?.map((album) => (
            <AlbumRow key={album.uri} album={album} tile={TILE} />
          ))}
        </Shelf>
        <Shelf title="Playlists" loading={playlists.isPending}>
          {playlists.data?.map((playlist) => (
            <PlaylistRow key={playlist.uri} playlist={playlist} tile={TILE} />
          ))}
        </Shelf>
        <Shelf title="Continue listening" loading={listening.isPending}>
          {listening.data?.map((book) => (
            <BookRow key={book.uri} book={book} tile={TILE} />
          ))}
        </Shelf>
        <Shelf title="Artists" loading={artists.isPending}>
          {artists.data?.map((artist) => (
            <ArtistRow key={artist.uri} artist={artist} tile={TILE} />
          ))}
        </Shelf>
        <Shelf
          title="Rediscover"
          loading={random.isPending}
          action={
            <Pressable
              onPress={() => setRound((r) => r + 1)}
              hitSlop={12}
              accessibilityRole="button"
              accessibilityLabel="Pick other albums"
              testID="home-reshuffle"
            >
              <Icon name="shuffle" size={18} color={theme.colors.accent} />
            </Pressable>
          }
        >
          {random.data?.map((album) => (
            <AlbumRow key={album.uri} album={album} tile={TILE} />
          ))}
        </Shelf>
      </ScrollView>
    </Screen>
  );
}

/** A titled row of tiles that scrolls sideways, with stand-ins while it loads. */
function Shelf({
  title,
  loading,
  action,
  children,
}: {
  title: string;
  loading: boolean;
  action?: ReactNode;
  children: ReactNode;
}) {
  const empty = !loading && (!children || (Array.isArray(children) && children.length === 0));
  if (empty) return null;
  return (
    <View style={styles.shelf}>
      <View style={styles.shelfHeader}>
        <Text variant="heading">{title}</Text>
        {action}
      </View>
      <ScrollView
        horizontal
        showsHorizontalScrollIndicator={false}
        contentContainerStyle={styles.tiles}
        style={styles.bleed}
      >
        {loading
          ? ['a', 'b', 'c'].map((key) => <View key={key} style={styles.placeholder} />)
          : children}
      </ScrollView>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  content: { paddingBottom: theme.spacing.xl },
  title: { marginVertical: theme.spacing.md },
  shelf: { marginTop: theme.spacing.lg },
  shelfHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    marginBottom: theme.spacing.md,
  },
  // Reaching the screen's edges, so tiles scroll out of view rather than into its padding.
  bleed: { marginHorizontal: -theme.spacing.lg },
  tiles: { gap: theme.spacing.md, paddingHorizontal: theme.spacing.lg },
  placeholder: {
    width: TILE,
    height: TILE,
    borderRadius: 6,
    backgroundColor: theme.colors.surfaceVariant,
  },
}));
