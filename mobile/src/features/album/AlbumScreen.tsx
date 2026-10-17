import { albumTracks, getAlbum } from '@bopsie/client/api/endpoints/library';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { LegendList } from '@legendapp/list/react-native';
import { useQuery } from '@tanstack/react-query';
import { Stack } from 'expo-router';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { ItemHeader } from '@/features/library/ItemHeader';
import { MoreButton } from '@/features/library/menu';
import { artistNames, TrackRow } from '@/features/library/rows';
import { useOpenItemMenu } from '@/features/library/useOpenItemMenu';
import { usePlaybackActions } from '@/features/player/usePlayback';
import { describeError } from '@/lib/errors';
import { InlineError } from '@/ui/primitives';

export function AlbumScreen({
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
  const ref = { itemId: id, provider };
  const album = useQuery({
    queryKey: queryKeys.item(account.id, 'album', id, provider),
    queryFn: () => getAlbum(connection, ref),
  });
  const tracks = useQuery({
    queryKey: [...queryKeys.item(account.id, 'album', id, provider), 'tracks'],
    queryFn: () => albumTracks(connection, ref),
  });
  const uri = album.data?.uri;
  const error = album.error ?? tracks.error;

  return (
    <View style={styles.screen}>
      <Stack.Screen
        options={{
          title: album.data?.name ?? name ?? '',
          headerRight: () => {
            const loaded = album.data;
            return loaded ? (
              <MoreButton onPress={() => openMenu('album', loaded)} testID="item-more" />
            ) : null;
          },
        }}
      />
      <LegendList
        data={tracks.data ?? []}
        keyExtractor={(track) => `${track.provider}/${track.item_id}`}
        renderItem={({ item, index }) => (
          <TrackRow
            track={item}
            index={index}
            onPress={() => (uri ? playback.play(uri, item.uri) : undefined)}
          />
        )}
        estimatedItemSize={56}
        ListHeaderComponent={
          <>
            <ItemHeader
              item={album.data ?? null}
              title={album.data?.name ?? name ?? ''}
              subtitle={
                album.data
                  ? [artistNames(album.data), album.data.year].filter(Boolean).join(' · ')
                  : ''
              }
              onPlay={() => (uri ? playback.play(uri) : undefined)}
              onShuffle={() => (uri ? playback.shufflePlay(uri) : undefined)}
            />
            {error ? <InlineError message={describeError(error)} /> : null}
          </>
        }
        contentContainerStyle={styles.content}
        testID="album-tracks"
      />
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  screen: { flex: 1, backgroundColor: theme.colors.background },
  content: { paddingHorizontal: theme.spacing.lg, paddingBottom: theme.spacing.xxl },
}));
