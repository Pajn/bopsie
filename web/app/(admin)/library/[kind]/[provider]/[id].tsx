import {
  albumTracks,
  artistAlbums,
  artistAudiobooks,
  artistTracks,
  getAlbum,
  getArtist,
  getAudiobook,
  getPlaylist,
  getTrack,
  playlistTracks,
} from '@bopsie/client/api/endpoints/library';
import type { Album, Audiobook, Track } from '@bopsie/client/api/schemas/media';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useLocalSearchParams } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import JsonTree from '@/debug/JsonTree';
import { useTreeColors } from '@/debug/treeColors';
import { ChapterList } from '@/features/audiobooks/BookControls';
import { EditItem } from '@/features/library/EditItem';
import { ItemGrid } from '@/features/library/ItemGrid';
import { ItemHero } from '@/features/library/ItemHero';
import type { Item } from '@/features/library/ItemRow';
import { Sources } from '@/features/library/Sources';
import { TrackList } from '@/features/library/TrackList';
import { describeError } from '@/lib/errors';
import { Disclosure } from '@/ui/controls';
import { Button, Card, InlineError, Page, Text } from '@/ui/primitives';
import { Skeleton, SkeletonRows } from '@/ui/Skeleton';

const KINDS = ['album', 'artist', 'playlist', 'track'] as const;

/** An artist's tracks shown before Show all. */
const ARTIST_TRACKS = 10;
type Kind = (typeof KINDS)[number];

/**
 * An album, artist, playlist or track: what it holds and plays, and beside that, or below it
 * on a narrower screen, where it comes from and its metadata to edit.
 */
export default function ItemScreen() {
  const { kind, provider, id } = useLocalSearchParams<{
    kind: string;
    provider: string;
    id: string;
  }>();
  const { account, connection } = useConnection();
  const colors = useTreeColors();
  const ref = { itemId: id, provider };
  const item = useQuery({
    queryKey: queryKeys.item(account.id, kind, id, provider),
    queryFn: (): Promise<Item> =>
      kind === 'album'
        ? getAlbum(connection, ref)
        : kind === 'audiobook'
          ? getAudiobook(connection, ref)
          : kind === 'artist'
            ? getArtist(connection, ref)
            : kind === 'track'
              ? getTrack(connection, ref)
              : getPlaylist(connection, ref),
  });
  const contents = useQuery({
    queryKey: [...queryKeys.item(account.id, kind, id, provider), 'contents'],
    queryFn: async (): Promise<{ albums: Item[]; tracks: Track[]; books: Item[] }> => {
      const none = { albums: [], tracks: [], books: [] };
      if (kind === 'track' || kind === 'audiobook') return none;
      if (kind === 'album') return { ...none, tracks: await albumTracks(connection, ref) };
      if (kind === 'playlist') return { ...none, tracks: await playlistTracks(connection, ref) };
      // An author is an artist of the source that serves their books.
      const [albums, tracks, books] = await Promise.all([
        artistAlbums(connection, ref),
        artistTracks(connection, ref),
        artistAudiobooks(connection, ref),
      ]);
      return { albums, tracks, books };
    },
  });
  const error = item.error ?? contents.error;
  const data = item.data;
  // Only the library's own items are edited; a catalog's, such as Spotify's, are as it says.
  const editable = data && provider === 'library' && KINDS.includes(kind as Kind);
  // An artist's albums newest first; an album's or a playlist's tracks in their order.
  const albums = [...(contents.data?.albums ?? [])].sort(
    (a, b) => ((b as Album).year ?? 0) - ((a as Album).year ?? 0),
  );
  const tracks = contents.data?.tracks ?? [];
  const books = contents.data?.books ?? [];
  const book = kind === 'audiobook' ? (data as Audiobook | undefined) : undefined;
  const chapters = book?.metadata.chapters ?? [];
  const [allTracks, setAllTracks] = useState(false);
  const shownTracks = kind === 'artist' && !allTracks ? tracks.slice(0, ARTIST_TRACKS) : tracks;

  return (
    <Page
      title={data?.name ?? id}
      back={{ href: '/library', label: 'Library' }}
      wide
      hero={
        data ? (
          <ItemHero item={data} tracks={kind === 'artist' ? undefined : tracks} />
        ) : item.isPending ? (
          <View style={styles.heroLoading}>
            <Skeleton width={200} height={200} round={kind === 'artist'} />
            <View style={styles.heroLines}>
              <Skeleton width={60} height={10} radius={4} />
              <Skeleton width="60%" height={32} radius={6} />
              <Skeleton width="30%" height={14} radius={4} />
            </View>
          </View>
        ) : null
      }
    >
      <InlineError message={error ? describeError(error) : null} />
      <View style={styles.columns}>
        <View style={styles.main}>
          {book?.metadata.description ? (
            <Card>
              <Text>{book.metadata.description}</Text>
            </Card>
          ) : null}
          {chapters.length > 0 && book ? (
            <Card title={`${chapters.length} ${chapters.length === 1 ? 'chapter' : 'chapters'}`}>
              <ChapterList chapters={chapters} bookSeconds={book.duration} />
            </Card>
          ) : null}
          {books.length > 0 ? (
            <View style={styles.section}>
              <Text variant="heading">Audiobooks</Text>
              <ItemGrid items={books} narrow />
            </View>
          ) : null}
          {albums.length > 0 ? (
            <View style={styles.section}>
              <Text variant="heading">Albums</Text>
              <ItemGrid
                items={albums}
                narrow
                caption={(album) => String((album as Album).year ?? '')}
              />
            </View>
          ) : null}
          {contents.isPending ? (
            <Card>
              <SkeletonRows count={8} art={null} />
            </Card>
          ) : null}
          {tracks.length > 0 ? (
            <Card {...(kind === 'artist' && { title: 'Tracks' })}>
              <TrackList
                tracks={shownTracks}
                from={kind === 'album' || kind === 'playlist' ? data?.uri : undefined}
                numbered={kind === 'album'}
                detail={kind === 'artist' ? 'album' : kind === 'album' ? null : 'artists'}
              />
              {shownTracks.length < tracks.length ? (
                <Button
                  title={`Show all ${tracks.length} tracks`}
                  variant="secondary"
                  onPress={() => setAllTracks(true)}
                />
              ) : null}
            </Card>
          ) : null}
        </View>
        {data ? (
          <View style={styles.side}>
            {provider === 'library' ? <Sources item={data} /> : null}
            {editable ? (
              <Disclosure title="Edit details" hint="Outlasts a rescan">
                <EditItem key={data.uri} item={data} kind={kind as Kind} />
              </Disclosure>
            ) : null}
            <Disclosure title="Raw" hint="What the server reports">
              <JsonTree value={JSON.parse(JSON.stringify(data))} colors={colors} />
            </Disclosure>
          </View>
        ) : null}
      </View>
    </Page>
  );
}

const styles = StyleSheet.create((theme) => ({
  columns: {
    flexDirection: { xs: 'column', lg: 'row' },
    alignItems: { xs: 'stretch', lg: 'flex-start' },
    gap: theme.spacing.lg,
  },
  // Grows beside the side column; above it, as tall as it holds.
  main: { flexGrow: 1, flexBasis: { xs: 'auto', lg: 0 }, minWidth: 0, gap: theme.spacing.lg },
  side: { width: { xs: 'auto', lg: 380 }, gap: theme.spacing.lg },
  section: { gap: theme.spacing.sm },
  heroLoading: {
    flexDirection: { xs: 'column', md: 'row' },
    alignItems: { xs: 'flex-start', md: 'flex-end' },
    gap: theme.spacing.xl,
  },
  heroLines: { flex: 1, width: '100%', gap: theme.spacing.sm },
}));
