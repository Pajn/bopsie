import { getPlaylist, playlistTracks } from '@bopsie/client/api/endpoints/library';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { LegendList } from '@legendapp/list/react-native';
import { useQuery } from '@tanstack/react-query';
import { Stack } from 'expo-router';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { ItemHeader } from '@/features/library/ItemHeader';
import { MoreButton } from '@/features/library/menu';
import { TrackRow } from '@/features/library/rows';
import { useOpenItemMenu } from '@/features/library/useOpenItemMenu';
import { usePlaybackActions } from '@/features/player/usePlayback';
import { describeError } from '@/lib/errors';
import { InlineError } from '@/ui/primitives';

export function PlaylistScreen({
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
  const playlist = useQuery({
    queryKey: queryKeys.item(account.id, 'playlist', id, provider),
    queryFn: () => getPlaylist(connection, ref),
  });
  const tracks = useQuery({
    queryKey: [...queryKeys.item(account.id, 'playlist', id, provider), 'tracks'],
    queryFn: () => playlistTracks(connection, ref),
  });
  const uri = playlist.data?.uri;
  const error = playlist.error ?? tracks.error;

  return (
    <View style={styles.screen}>
      <Stack.Screen
        options={{
          title: playlist.data?.name ?? name ?? '',
          headerRight: () => {
            const loaded = playlist.data;
            return loaded ? (
              <MoreButton onPress={() => openMenu('playlist', loaded)} testID="item-more" />
            ) : null;
          },
        }}
      />
      <LegendList
        data={tracks.data ?? []}
        keyExtractor={(track, index) => `${index}/${track.provider}/${track.item_id}`}
        renderItem={({ item }) => (
          <TrackRow track={item} showArtwork onPress={() => uri && playback.play(uri, item.uri)} />
        )}
        estimatedItemSize={56}
        ListHeaderComponent={
          <>
            <ItemHeader
              item={playlist.data ?? null}
              title={playlist.data?.name ?? name ?? ''}
              {...(playlist.data?.owner ? { subtitle: playlist.data.owner } : {})}
              onPlay={() => uri && playback.play(uri)}
              placeholder="list"
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
