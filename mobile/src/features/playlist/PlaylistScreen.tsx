import { LegendList } from '@legendapp/list/react-native';
import { useQuery } from '@tanstack/react-query';
import { Stack } from 'expo-router';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { getPlaylist, playlistTracks } from '@/api/endpoints/library';
import { useConnection } from '@/connection/ConnectionProvider';
import { queryKeys } from '@/connection/queryKeys';
import { ItemHeader } from '@/features/library/ItemHeader';
import { TrackRow } from '@/features/library/rows';
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
      <Stack.Screen options={{ title: playlist.data?.name ?? name ?? '' }} />
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
