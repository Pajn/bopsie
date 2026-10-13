import { LegendList } from '@legendapp/list/react-native';
import { useQuery } from '@tanstack/react-query';
import { Stack } from 'expo-router';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { artistAlbums, getArtist } from '@/api/endpoints/library';
import { useConnection } from '@/connection/ConnectionProvider';
import { queryKeys } from '@/connection/queryKeys';
import { ItemHeader } from '@/features/library/ItemHeader';
import { AlbumRow } from '@/features/library/rows';
import { usePlaybackActions } from '@/features/player/usePlayback';
import { describeError } from '@/lib/errors';
import { InlineError } from '@/ui/primitives';

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
  const ref = { itemId: id, provider };
  const artist = useQuery({
    queryKey: queryKeys.item(account.id, 'artist', id, provider),
    queryFn: () => getArtist(connection, ref),
  });
  const albums = useQuery({
    queryKey: [...queryKeys.item(account.id, 'artist', id, provider), 'albums'],
    queryFn: () => artistAlbums(connection, ref),
  });
  const error = artist.error ?? albums.error;

  return (
    <View style={styles.screen}>
      <Stack.Screen options={{ title: artist.data?.name ?? name ?? '' }} />
      <LegendList
        data={albums.data ?? []}
        keyExtractor={(album) => `${album.provider}/${album.item_id}`}
        renderItem={({ item }) => <AlbumRow album={item} />}
        estimatedItemSize={64}
        ListHeaderComponent={
          <>
            <ItemHeader
              item={artist.data ?? null}
              title={artist.data?.name ?? name ?? ''}
              onPlay={() => artist.data && playback.play(artist.data.uri)}
              round
              placeholder="user"
            />
            {error ? <InlineError message={describeError(error)} /> : null}
          </>
        }
        contentContainerStyle={styles.content}
      />
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  screen: { flex: 1, backgroundColor: theme.colors.background },
  content: { paddingHorizontal: theme.spacing.lg, paddingBottom: theme.spacing.xxl },
}));
