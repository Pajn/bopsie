import type { Album, Artist, Playlist, Track } from '@bopsie/client/api/schemas/media';
import { useRouter } from 'expo-router';
import { Pressable, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { PlayingBars, usePlayingState } from '@/features/player/PlayingBars';
import { sourceName } from '@/features/search/scope';
import { useProviders } from '@/features/search/useProviders';
import { formatDuration } from '@/player/progress';
import { Text } from '@/ui/primitives';
import { Artwork } from './Artwork';
import { MoreButton } from './menu';
import { useOpenItemMenu } from './useOpenItemMenu';

export const artistNames = (item: { artists: Array<{ name: string }> }) =>
  item.artists.map((artist) => artist.name).join(', ');

/** A subtitle led by where the item comes from, when that is not the library: "Spotify · …". */
function useMarked(provider: string, ...parts: Array<string | number | null | undefined>): string {
  const providers = useProviders().data;
  return [sourceName(providers, provider), ...parts].filter(Boolean).join(' · ');
}

/** A row in a list. A long press, or the more button where there is one, opens its menu. */
function Row({
  onPress,
  onMenu,
  moreButton,
  testID,
  artwork,
  title,
  subtitle,
  trailing,
  current,
}: {
  onPress: () => void;
  onMenu: () => void;
  moreButton?: boolean;
  testID: string;
  artwork?: React.ReactNode;
  title: string;
  subtitle?: string;
  trailing?: string;
  /** What plays now, which the title marks. */
  current?: boolean;
}) {
  return (
    <Pressable
      onPress={onPress}
      onLongPress={onMenu}
      style={({ pressed }) => [styles.row, pressed && styles.pressed]}
      accessibilityRole="button"
      accessibilityLabel={title}
      testID={testID}
    >
      {artwork}
      <View style={styles.text}>
        <Text numberOfLines={1} style={current && styles.current}>
          {title}
        </Text>
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
      {moreButton ? (
        <MoreButton
          onPress={onMenu}
          label={`More for ${title}`}
          vertical
          testID={`${testID}-more`}
        />
      ) : null}
    </Pressable>
  );
}

export function ArtistRow({ artist }: { artist: Artist }) {
  const router = useRouter();
  const openMenu = useOpenItemMenu();
  const subtitle = useMarked(artist.provider);
  return (
    <Row
      onPress={() =>
        router.push({
          pathname: '/artist/[id]',
          params: { id: artist.item_id, provider: artist.provider, name: artist.name },
        })
      }
      onMenu={() => openMenu('artist', artist)}
      testID={`artist-${artist.item_id}`}
      artwork={<Artwork item={artist} size={48} imageSize={80} round placeholder="user" />}
      title={artist.name}
      {...(subtitle ? { subtitle } : {})}
    />
  );
}

export function AlbumRow({ album }: { album: Album }) {
  const router = useRouter();
  const openMenu = useOpenItemMenu();
  const subtitle = useMarked(album.provider, artistNames(album), album.year);
  return (
    <Row
      onPress={() =>
        router.push({
          pathname: '/album/[id]',
          params: { id: album.item_id, provider: album.provider, name: album.name },
        })
      }
      onMenu={() => openMenu('album', album)}
      testID={`album-${album.item_id}`}
      artwork={<Artwork item={album} size={48} imageSize={80} />}
      title={album.name}
      subtitle={subtitle}
    />
  );
}

export function PlaylistRow({ playlist }: { playlist: Playlist }) {
  const router = useRouter();
  const openMenu = useOpenItemMenu();
  const subtitle = useMarked(playlist.provider, playlist.owner);
  return (
    <Row
      onPress={() =>
        router.push({
          pathname: '/playlist/[id]',
          params: { id: playlist.item_id, provider: playlist.provider, name: playlist.name },
        })
      }
      onMenu={() => openMenu('playlist', playlist)}
      testID={`playlist-${playlist.item_id}`}
      artwork={<Artwork item={playlist} size={48} imageSize={80} placeholder="list" />}
      title={playlist.name}
      {...(subtitle ? { subtitle } : {})}
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
  const subtitle = useMarked(track.provider, artistNames(track));
  const openMenu = useOpenItemMenu();
  const playing = usePlayingState(track.uri);
  return (
    <Row
      onPress={onPress}
      onMenu={() => openMenu('track', track)}
      moreButton
      testID={`track-${track.item_id}`}
      artwork={
        showArtwork ? (
          <View>
            <Artwork item={track.album ?? track} size={40} imageSize={80} />
            {playing ? (
              <View style={styles.scrim}>
                <PlayingBars playing={playing === 'playing'} color="#FFFFFF" />
              </View>
            ) : null}
          </View>
        ) : playing ? (
          <View style={styles.number}>
            <PlayingBars playing={playing === 'playing'} />
          </View>
        ) : (
          <Text muted style={styles.number}>
            {track.track_number ?? (index !== undefined ? index + 1 : '')}
          </Text>
        )
      }
      current={playing !== null}
      title={track.name}
      subtitle={subtitle}
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
  number: { width: 28, textAlign: 'center', alignItems: 'center' },
  current: { color: theme.colors.accent, fontWeight: '600' },
  // Over the artwork of what plays, so the bars read on any cover.
  scrim: {
    ...StyleSheet.absoluteFillObject,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 6,
    backgroundColor: 'rgba(0, 0, 0, 0.45)',
  },
}));
