import { imageUrl } from '@bopsie/client/api/images';
import type { Album, Playlist, Track } from '@bopsie/client/api/schemas/media';
import { Link } from 'expo-router';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { duration } from '@/lib/time';
import { Artwork } from '@/ui/Artwork';
import { Text } from '@/ui/primitives';
import type { Item } from './ItemRow';
import { itemHref } from './links';
import { PlayActions } from './PlayButtons';

const KIND = { album: 'Album', artist: 'Artist', playlist: 'Playlist', track: 'Track' } as const;

/** Who made it, as links to their pages where the library has them. */
function Artists({ item }: { item: Item }) {
  const artists = 'artists' in item ? (item as Album | Track).artists : [];
  if (artists.length === 0) return null;
  return (
    <View style={styles.artists}>
      {artists.map((artist, i) => (
        <Link key={`${artist.provider}-${artist.item_id}`} href={itemHref(artist) as never} asChild>
          <Pressable accessibilityRole="link">
            <Text style={styles.artist}>
              {artist.name}
              {i < artists.length - 1 ? ',' : ''}
            </Text>
          </Pressable>
        </Link>
      ))}
    </View>
  );
}

/** What an item is and holds, in a line: its year, its tracks and how long they run. */
function details(item: Item, tracks: Track[] | undefined): string {
  const parts: string[] = [];
  if (item.media_type === 'album' && (item as Album).year) parts.push(String((item as Album).year));
  if (item.media_type === 'track') {
    const track = item as Track;
    if (track.album?.name) parts.push(track.album.name);
    if (track.duration) parts.push(duration(track.duration));
  }
  if (item.media_type === 'playlist' && (item as Playlist).owner) {
    parts.push(`By ${(item as Playlist).owner}`);
  }
  if (tracks?.length) {
    parts.push(`${tracks.length} ${tracks.length === 1 ? 'track' : 'tracks'}`);
    const total = tracks.reduce((sum, track) => sum + (track.duration ?? 0), 0);
    if (total > 0) parts.push(duration(total));
  }
  return parts.join(' · ');
}

/** The head of an item's page: its artwork, what it is, who made it, and Play. */
export function ItemHero({ item, tracks }: { item: Item; tracks?: Track[] | undefined }) {
  const { account } = useConnection();
  const { rt } = useUnistyles();
  const wide = rt.breakpoint !== 'xs';
  const round = item.media_type === 'artist';
  return (
    <View style={styles.hero}>
      <Artwork
        uri={imageUrl(account.serverUrl, item, 512)}
        size={wide ? 200 : 160}
        round={round}
        placeholder={round ? 'user' : item.media_type === 'playlist' ? 'list' : 'disc'}
      />
      <View style={styles.text}>
        <Text variant="label" muted>
          {KIND[item.media_type as keyof typeof KIND]?.toUpperCase() ?? item.media_type}
        </Text>
        <Text style={styles.name} numberOfLines={3}>
          {item.name}
        </Text>
        <Artists item={item} />
        <Text muted>{details(item, tracks)}</Text>
        <View style={styles.actions}>
          <PlayActions uri={item.uri} />
        </View>
      </View>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  hero: {
    flexDirection: { xs: 'column', md: 'row' },
    alignItems: { xs: 'flex-start', md: 'flex-end' },
    gap: theme.spacing.xl,
  },
  text: { flex: 1, minWidth: 0, gap: theme.spacing.xs },
  name: {
    ...theme.typography.title,
    fontSize: { xs: 26, md: 34 },
    lineHeight: { xs: 32, md: 40 },
    fontWeight: '700',
    color: theme.colors.text,
  },
  artists: { flexDirection: 'row', flexWrap: 'wrap', gap: theme.spacing.xs },
  artist: { fontWeight: '600', color: theme.colors.accent },
  actions: { paddingTop: theme.spacing.sm },
}));
