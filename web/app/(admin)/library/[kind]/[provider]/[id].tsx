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
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useLocalSearchParams } from 'expo-router';
import { ScrollView } from 'react-native';
import { useConnection } from '@/connection/ConnectionProvider';
import JsonTree from '@/debug/JsonTree';
import { useTreeColors } from '@/debug/treeColors';
import { EditItem } from '@/features/library/EditItem';
import { type Item, ItemRow } from '@/features/library/ItemRow';
import { PlayButtons, TargetPicker } from '@/features/library/PlayButtons';
import { Sources } from '@/features/library/Sources';
import { describeError } from '@/lib/errors';
import { Card, InlineError, Stack, Text } from '@/ui/primitives';

const KINDS = ['album', 'artist', 'playlist', 'track'] as const;
type Kind = (typeof KINDS)[number];

/** An album, artist, playlist or track: what it holds, and its metadata to edit. */
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
    queryFn: async (): Promise<{ albums: Item[]; tracks: Item[] }> => {
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
  const uri = item.data?.uri;

  return (
    <ScrollView>
      <Stack gap="lg">
        <Text variant="title">{item.data?.name ?? id}</Text>
        <TargetPicker />
        <InlineError message={error ? describeError(error) : null} />
        {item.data && uri ? <ItemRow item={item.data} actions={<PlayButtons uri={uri} />} /> : null}
        {/* Only the library's own items are edited; a catalog's, such as Spotify's, are as it says. */}
        {item.data && provider === 'library' && KINDS.includes(kind as Kind) ? (
          <EditItem key={item.data.uri} item={item.data} kind={kind as Kind} />
        ) : null}
        {item.data && provider === 'library' ? <Sources item={item.data} /> : null}
        {contents.data?.albums.length ? (
          <Card title="Albums">
            {contents.data.albums.map((album) => (
              <ItemRow key={album.uri} item={album} actions={<PlayButtons uri={album.uri} />} />
            ))}
          </Card>
        ) : null}
        {contents.data?.tracks.length ? (
          <Card title="Tracks">
            {contents.data.tracks.map((track) => (
              <ItemRow
                key={track.uri}
                item={track}
                actions={
                  // Inside an album or playlist, play it from this track on.
                  uri && kind !== 'artist' ? (
                    <PlayButtons uri={uri} startItem={track.item_id} />
                  ) : (
                    <PlayButtons uri={track.uri} />
                  )
                }
              />
            ))}
          </Card>
        ) : null}
        {item.data ? (
          <Card title="Raw">
            <JsonTree value={JSON.parse(JSON.stringify(item.data))} colors={colors} />
          </Card>
        ) : null}
      </Stack>
    </ScrollView>
  );
}
