import { useRouter } from 'expo-router';
import { Pressable, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import type { Album, Artist, Playlist, Track } from '@/api/schemas/media';
import { formatDuration } from '@/player/progress';
import { Text } from '@/ui/primitives';
import { Artwork } from './Artwork';

export const artistNames = (item: { artists: Array<{ name: string }> }) =>
  item.artists.map((artist) => artist.name).join(', ');

function Row({
  onPress,
  testID,
  artwork,
  title,
  subtitle,
  trailing,
}: {
  onPress: () => void;
  testID: string;
  artwork?: React.ReactNode;
  title: string;
  subtitle?: string;
  trailing?: string;
}) {
  return (
    <Pressable
      onPress={onPress}
      style={({ pressed }) => [styles.row, pressed && styles.pressed]}
      accessibilityRole="button"
      accessibilityLabel={title}
      testID={testID}
    >
      {artwork}
      <View style={styles.text}>
        <Text numberOfLines={1}>{title}</Text>
        {subtitle ? (
          <Text variant="caption" muted numberOfLines={1}>
            {subtitle}
          </Text>
        ) : null}
      </View>
      {trailing ? (
        <Text variant="caption" muted>
          {trailing}
        </Text>
      ) : null}
    </Pressable>
  );
}

export function ArtistRow({ artist }: { artist: Artist }) {
  const router = useRouter();
  return (
    <Row
      onPress={() =>
        router.push({
          pathname: '/artist/[id]',
          params: { id: artist.item_id, provider: artist.provider, name: artist.name },
        })
      }
      testID={`artist-${artist.item_id}`}
      artwork={<Artwork item={artist} size={48} imageSize={80} round placeholder="user" />}
      title={artist.name}
    />
  );
}

export function AlbumRow({ album }: { album: Album }) {
  const router = useRouter();
  return (
    <Row
      onPress={() =>
        router.push({
          pathname: '/album/[id]',
          params: { id: album.item_id, provider: album.provider, name: album.name },
        })
      }
      testID={`album-${album.item_id}`}
      artwork={<Artwork item={album} size={48} imageSize={80} />}
      title={album.name}
      subtitle={[artistNames(album), album.year].filter(Boolean).join(' · ')}
    />
  );
}

export function PlaylistRow({ playlist }: { playlist: Playlist }) {
  const router = useRouter();
  return (
    <Row
      onPress={() =>
        router.push({
          pathname: '/playlist/[id]',
          params: { id: playlist.item_id, provider: playlist.provider, name: playlist.name },
        })
      }
      testID={`playlist-${playlist.item_id}`}
      artwork={<Artwork item={playlist} size={48} imageSize={80} placeholder="list" />}
      title={playlist.name}
      {...(playlist.owner ? { subtitle: playlist.owner } : {})}
    />
  );
}

/** A track in a list. `onPress` plays it, usually within its album or playlist. */
export function TrackRow({
  track,
  onPress,
  showArtwork,
  index,
}: {
  track: Track;
  onPress: () => void;
  showArtwork?: boolean;
  index?: number;
}) {
  return (
    <Row
      onPress={onPress}
      testID={`track-${track.item_id}`}
      artwork={
        showArtwork ? (
          <Artwork item={track.album ?? track} size={40} imageSize={80} />
        ) : (
          <Text muted style={styles.number}>
            {track.track_number ?? (index !== undefined ? index + 1 : '')}
          </Text>
        )
      }
      title={track.name}
      subtitle={artistNames(track)}
      {...(track.duration ? { trailing: formatDuration(track.duration) } : {})}
    />
  );
}

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingVertical: theme.spacing.sm,
  },
  pressed: { opacity: 0.6 },
  text: { flex: 1, gap: 2 },
  number: { width: 28, textAlign: 'center' },
}));
