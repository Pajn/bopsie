import type { Album, Artist, Audiobook, Playlist, Track } from '@bopsie/client/api/schemas/media';
import { useRouter } from 'expo-router';
import { ActivityIndicator, Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { bookProgress } from '@/features/audiobooks/model';
import { PlayingBars, usePlayingState } from '@/features/player/PlayingBars';
import { sourceName } from '@/features/search/scope';
import { useProviders } from '@/features/search/useProviders';
import { formatDuration } from '@/player/progress';
import { haptics } from '@/ui/haptics';
import { Text } from '@/ui/primitives';
import { usePending } from '@/ui/usePending';
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
  busy,
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
  /** What the row's press started is still starting. */
  busy?: boolean;
}) {
  return (
    <Pressable
      onPress={onPress}
      onLongPress={() => {
        haptics.longPress();
        onMenu();
      }}
      style={({ pressed }) => [styles.row, pressed && styles.pressed]}
      accessibilityRole="button"
      accessibilityLabel={title}
      accessibilityState={{ busy: Boolean(busy) }}
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

/**
 * An item in a grid: its artwork `size` wide, and its name and subtitle below. A long press
 * opens its menu, as a row's does.
 */
function Tile({
  onPress,
  onMenu,
  testID,
  item,
  size,
  round,
  placeholder,
  title,
  subtitle,
}: {
  onPress: () => void;
  onMenu: () => void;
  testID: string;
  item: Parameters<typeof Artwork>[0]['item'];
  size: number;
  round?: boolean;
  placeholder?: Parameters<typeof Artwork>[0]['placeholder'];
  title: string;
  subtitle?: string;
}) {
  return (
    <Pressable
      onPress={onPress}
      onLongPress={() => {
        haptics.longPress();
        onMenu();
      }}
      style={({ pressed }) => [styles.tile, { width: size }, pressed && styles.pressed]}
      accessibilityRole="button"
      accessibilityLabel={title}
      testID={testID}
    >
      <Artwork
        item={item}
        size={size}
        imageSize={size > 160 ? 512 : 256}
        {...(round !== undefined && { round })}
        {...(placeholder && { placeholder })}
      />
      <View style={[styles.tileText, round && styles.centred]}>
        <Text numberOfLines={1} style={round && styles.centredText}>
          {title}
        </Text>
        {subtitle ? (
          <Text variant="caption" muted numberOfLines={1} style={round && styles.centredText}>
            {subtitle}
          </Text>
        ) : null}
      </View>
    </Pressable>
  );
}

/** An artist, album or playlist as a row, or, given `tile`, as a grid tile that wide. */
interface ItemProps {
  tile?: number;
}

export function ArtistRow({ artist, tile }: { artist: Artist } & ItemProps) {
  const router = useRouter();
  const openMenu = useOpenItemMenu();
  const subtitle = useMarked(artist.provider);
  const open = () =>
    router.push({
      pathname: '/artist/[id]',
      params: { id: artist.item_id, provider: artist.provider, name: artist.name },
    });
  const menu = () => openMenu('artist', artist);
  if (tile) {
    return (
      <Tile
        onPress={open}
        onMenu={menu}
        testID={`artist-${artist.item_id}`}
        item={artist}
        size={tile}
        round
        placeholder="user"
        title={artist.name}
        {...(subtitle ? { subtitle } : {})}
      />
    );
  }
  return (
    <Row
      onPress={open}
      onMenu={menu}
      testID={`artist-${artist.item_id}`}
      artwork={<Artwork item={artist} size={48} imageSize={80} round placeholder="user" />}
      title={artist.name}
      {...(subtitle ? { subtitle } : {})}
    />
  );
}

export function AlbumRow({ album, tile }: { album: Album } & ItemProps) {
  const router = useRouter();
  const openMenu = useOpenItemMenu();
  const subtitle = useMarked(album.provider, artistNames(album), album.year);
  const open = () =>
    router.push({
      pathname: '/album/[id]',
      params: { id: album.item_id, provider: album.provider, name: album.name },
    });
  const menu = () => openMenu('album', album);
  if (tile) {
    return (
      <Tile
        onPress={open}
        onMenu={menu}
        testID={`album-${album.item_id}`}
        item={album}
        size={tile}
        title={album.name}
        {...(subtitle ? { subtitle } : {})}
      />
    );
  }
  return (
    <Row
      onPress={open}
      onMenu={menu}
      testID={`album-${album.item_id}`}
      artwork={<Artwork item={album} size={48} imageSize={80} />}
      title={album.name}
      subtitle={subtitle}
    />
  );
}

export function PlaylistRow({ playlist, tile }: { playlist: Playlist } & ItemProps) {
  const router = useRouter();
  const openMenu = useOpenItemMenu();
  const subtitle = useMarked(playlist.provider, playlist.owner);
  const open = () =>
    router.push({
      pathname: '/playlist/[id]',
      params: { id: playlist.item_id, provider: playlist.provider, name: playlist.name },
    });
  const menu = () => openMenu('playlist', playlist);
  if (tile) {
    return (
      <Tile
        onPress={open}
        onMenu={menu}
        testID={`playlist-${playlist.item_id}`}
        item={playlist}
        size={tile}
        placeholder="list"
        title={playlist.name}
        {...(subtitle ? { subtitle } : {})}
      />
    );
  }
  return (
    <Row
      onPress={open}
      onMenu={menu}
      testID={`playlist-${playlist.item_id}`}
      artwork={<Artwork item={playlist} size={48} imageSize={80} placeholder="list" />}
      title={playlist.name}
      {...(subtitle ? { subtitle } : {})}
    />
  );
}

/** An audiobook, opening its page: who wrote it, and how far into it listening is. */
export function BookRow({ book, tile }: { book: Audiobook } & ItemProps) {
  const router = useRouter();
  const subtitle = [book.authors.map((author) => author.name).join(', '), bookProgress(book)]
    .filter(Boolean)
    .join(' · ');
  const open = () =>
    router.push({
      pathname: '/audiobook/[id]',
      params: { id: book.item_id, provider: book.provider, name: book.name },
    });
  if (tile) {
    return (
      <Tile
        onPress={open}
        onMenu={open}
        testID={`audiobook-${book.item_id}`}
        item={book}
        size={tile}
        placeholder="book"
        title={book.name}
        {...(subtitle ? { subtitle } : {})}
      />
    );
  }
  return (
    <Row
      onPress={open}
      onMenu={open}
      testID={`audiobook-${book.item_id}`}
      artwork={<Artwork item={book} size={48} imageSize={80} placeholder="book" />}
      title={book.name}
      {...(subtitle ? { subtitle } : {})}
    />
  );
}

/**
 * A track in a list. `onPress` plays it, usually within its album or playlist; until that has
 * started, a spinner stands where the playing bars will.
 */
export function TrackRow({
  track,
  onPress,
  showArtwork,
  index,
}: {
  track: Track;
  onPress: () => Promise<unknown> | undefined;
  showArtwork?: boolean;
  index?: number;
}) {
  const { theme } = useUnistyles();
  const subtitle = useMarked(track.provider, artistNames(track));
  const openMenu = useOpenItemMenu();
  const playing = usePlayingState(track.uri);
  const play = usePending(onPress);
  return (
    <Row
      onPress={play.onPress}
      busy={play.pending}
      onMenu={() => openMenu('track', track)}
      moreButton
      testID={`track-${track.item_id}`}
      artwork={
        showArtwork ? (
          <View>
            <Artwork item={track.album ?? track} size={40} imageSize={80} />
            {play.pending ? (
              <View style={styles.scrim}>
                <ActivityIndicator size="small" color="#FFFFFF" />
              </View>
            ) : playing ? (
              <View style={styles.scrim}>
                <PlayingBars playing={playing === 'playing'} color="#FFFFFF" />
              </View>
            ) : null}
          </View>
        ) : play.pending ? (
          <View style={styles.number}>
            <ActivityIndicator size="small" color={theme.colors.accent} />
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
  tile: { gap: theme.spacing.sm, marginBottom: theme.spacing.lg },
  tileText: { gap: 2 },
  centred: { alignItems: 'center' },
  centredText: { textAlign: 'center' },
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
