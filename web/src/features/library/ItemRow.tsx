import { imageUrl } from '@bopsie/client/api/images';
import type { Album, Artist, Playlist, Track } from '@bopsie/client/api/schemas/media';
import { Link } from 'expo-router';
import type { ReactNode } from 'react';
import { Image, Pressable, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { Text } from '@/ui/primitives';

export type Item = Album | Artist | Playlist | Track;

export function subtitle(item: Item): string {
  const names = (artists: Album['artists']) => artists.map((a) => a.name).join(', ');
  if (item.media_type === 'album') {
    const album = item as Album;
    return [names(album.artists), album.year].filter(Boolean).join(' · ');
  }
  if (item.media_type === 'track') {
    const track = item as Track;
    return [names(track.artists), track.album?.name].filter(Boolean).join(' · ');
  }
  return '';
}

/** Where an item opens. */
export function itemHref(item: Item): string {
  return `/library/${item.media_type}/${encodeURIComponent(item.provider)}/${encodeURIComponent(item.item_id)}`;
}

export function ItemRow({ item, actions }: { item: Item; actions?: ReactNode }) {
  const { account } = useConnection();
  const artwork = imageUrl(account.serverUrl, item, 80);
  return (
    <View style={styles.row}>
      {artwork ? (
        <Image source={{ uri: artwork }} style={styles.art} />
      ) : (
        <View style={styles.art} />
      )}
      <Link href={itemHref(item)} asChild>
        <Pressable style={styles.text}>
          <Text numberOfLines={1} style={styles.link}>
            {item.name}
          </Text>
          <Text variant="caption" muted numberOfLines={1}>
            {subtitle(item) || item.uri}
          </Text>
        </Pressable>
      </Link>
      {actions}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingVertical: theme.spacing.xs,
    borderBottomWidth: 1,
    borderColor: theme.colors.border,
  },
  art: {
    width: 40,
    height: 40,
    borderRadius: theme.radius.sm,
    backgroundColor: theme.colors.surfaceVariant,
  },
  text: { flex: 1, minWidth: 0 },
  link: { color: theme.colors.accent },
}));
