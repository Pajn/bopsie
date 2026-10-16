import { imageUrl } from '@bopsie/client/api/images';
import { Link } from 'expo-router';
import { useState } from 'react';
import { Image, Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { Icon } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { type Item, itemHref, subtitle } from './ItemRow';
import { PlayButtons } from './PlayButtons';

const PLACEHOLDER = { artist: 'user', playlist: 'list', track: 'music', album: 'disc' } as const;

/** An item as a tile: its artwork, its name and who made it, opening its page. */
function Tile({ item }: { item: Item }) {
  const { account } = useConnection();
  const { theme } = useUnistyles();
  const [hovered, setHovered] = useState(false);
  const artwork = imageUrl(account.serverUrl, item, 256);
  const round = item.media_type === 'artist';
  return (
    <View
      style={styles.cell}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
    >
      <Link href={itemHref(item) as never} asChild>
        <Pressable accessibilityRole="link" accessibilityLabel={item.name}>
          {/* Styled inside: the link merges its child's style, which loses unistyles' classes. */}
          <View style={styles.tile(hovered)}>
            <View style={styles.cover(round)}>
              {artwork ? (
                <Image source={{ uri: artwork }} style={styles.image} resizeMode="cover" />
              ) : (
                <Icon
                  name={PLACEHOLDER[item.media_type as keyof typeof PLACEHOLDER] ?? 'disc'}
                  size={36}
                  color={theme.colors.textMuted}
                />
              )}
            </View>
            <View style={styles.caption(round)}>
              <Text numberOfLines={1} style={styles.name}>
                {item.name}
              </Text>
              <Text variant="caption" muted numberOfLines={1}>
                {subtitle(item) || ' '}
              </Text>
            </View>
          </View>
        </Pressable>
      </Link>
      {/* Play and add sit over the cover while the pointer is on it; a phone opens the page. */}
      {hovered && !round ? (
        <View style={styles.overlay}>
          <PlayButtons uri={item.uri} />
        </View>
      ) : null}
    </View>
  );
}

/** Items as a grid of tiles, more of them across the wider the screen. */
export function ItemGrid({ items }: { items: Item[] }) {
  return (
    <View style={styles.grid}>
      {items.map((item) => (
        <Tile key={item.uri} item={item} />
      ))}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  grid: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    marginHorizontal: -theme.spacing.xs,
  },
  cell: {
    width: { xs: '50%', md: '25%', lg: '20%' },
    padding: theme.spacing.xs,
  },
  tile: (hovered: boolean) => ({
    gap: theme.spacing.sm,
    padding: theme.spacing.sm,
    borderRadius: theme.radius.lg,
    backgroundColor: hovered ? theme.colors.surfaceVariant : 'transparent',
  }),
  cover: (round: boolean) => ({
    width: '100%',
    aspectRatio: 1,
    borderRadius: round ? 9999 : theme.radius.md,
    overflow: 'hidden',
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor: theme.colors.surfaceVariant,
  }),
  image: { width: '100%', height: '100%' },
  caption: (round: boolean) => ({ alignItems: round ? 'center' : 'flex-start' }),
  name: { fontWeight: '600' },
  overlay: {
    position: 'absolute',
    right: theme.spacing.lg,
    // Over the cover's foot: the caption's two lines and the padding below it.
    bottom: 64,
    borderRadius: theme.radius.md,
    backgroundColor: theme.colors.surface,
    borderWidth: 1,
    borderColor: theme.colors.border,
  },
}));
