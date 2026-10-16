import {
  albumTracks,
  artistAlbums,
  artistTracks,
  getAlbum,
  getArtist,
  getPlaylist,
  getTrack,
  playlistTracks,
} from '@bopsie/client/api/endpoints/library';
import type { Album, Track } from '@bopsie/client/api/schemas/media';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useLocalSearchParams } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import JsonTree from '@/debug/JsonTree';
import { useTreeColors } from '@/debug/treeColors';
import { EditItem } from '@/features/library/EditItem';
import { ItemGrid } from '@/features/library/ItemGrid';
import { ItemHero } from '@/features/library/ItemHero';
import type { Item } from '@/features/library/ItemRow';
import { Sources } from '@/features/library/Sources';
import { TrackList } from '@/features/library/TrackList';
import { describeError } from '@/lib/errors';
import { Disclosure } from '@/ui/controls';
import { Button, Card, InlineError, Page, Text } from '@/ui/primitives';

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
        : kind === 'artist'
          ? getArtist(connection, ref)
          : kind === 'track'
            ? getTrack(connection, ref)
            : getPlaylist(connection, ref),
  });
  const contents = useQuery({
    queryKey: [...queryKeys.item(account.id, kind, id, provider), 'contents'],
    queryFn: async (): Promise<{ albums: Item[]; tracks: Track[] }> => {
      if (kind === 'track') return { albums: [], tracks: [] };
      if (kind === 'album') return { albums: [], tracks: await albumTracks(connection, ref) };
      if (kind === 'playlist') return { albums: [], tracks: await playlistTracks(connection, ref) };
      const [albums, tracks] = await Promise.all([
        artistAlbums(connection, ref),
        artistTracks(connection, ref),
      ]);
      return { albums, tracks };
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
  const [allTracks, setAllTracks] = useState(false);
  const shownTracks = kind === 'artist' && !allTracks ? tracks.slice(0, ARTIST_TRACKS) : tracks;

  return (
    <Page
      title={data?.name ?? id}
      back={{ href: '/library', label: 'Library' }}
      wide
      hero={data ? <ItemHero item={data} tracks={kind === 'artist' ? undefined : tracks} /> : null}
    >
      <InlineError message={error ? describeError(error) : null} />
      <View style={styles.columns}>
        <View style={styles.main}>
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
}));
