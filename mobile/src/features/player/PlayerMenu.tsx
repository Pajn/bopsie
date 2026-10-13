import { useRouter } from 'expo-router';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { Artwork } from '@/features/library/Artwork';
import { Icon } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { type ItemLink, itemLinks, nowPlaying } from './nowPlaying';
import { useQueue } from './usePlayback';

/**
 * What the full player's menu offers for the current track: its album and
 * each of its artists. Choosing one closes the player and opens that page.
 */
export function PlayerMenu() {
  const router = useRouter();
  const queue = useQueue().data;
  const item = queue?.current_item;
  const current = queue ? nowPlaying(queue) : null;
  if (!item || !current) return null;

  const open = (link: ItemLink) => {
    // Back to the tabs, closing this sheet and the player, then on to the page.
    router.dismissAll();
    router.push({
      pathname: link.kind === 'album' ? '/album/[id]' : '/artist/[id]',
      params: { id: link.id, provider: link.provider, name: link.name },
    });
  };

  return (
    <View style={styles.sheet} testID="player-menu">
      <View style={styles.track}>
        <Artwork item={current.artwork} size={48} imageSize={80} />
        <View style={styles.text}>
          <Text numberOfLines={1}>{current.title}</Text>
          {current.subtitle ? (
            <Text variant="caption" muted numberOfLines={1}>
              {current.subtitle}
            </Text>
          ) : null}
        </View>
      </View>
      {itemLinks(item).map((link) => (
        <MenuRow key={`${link.kind}-${link.provider}-${link.id}`} link={link} onPress={open} />
      ))}
    </View>
  );
}

function MenuRow({ link, onPress }: { link: ItemLink; onPress: (link: ItemLink) => void }) {
  const { theme } = useUnistyles();
  const label = link.kind === 'album' ? 'Go to album' : 'Go to artist';
  return (
    <Pressable
      onPress={() => onPress(link)}
      style={({ pressed }) => [styles.row, pressed && styles.pressed]}
      accessibilityRole="button"
      accessibilityLabel={`${label}: ${link.name}`}
      testID={`player-menu-${link.kind}-${link.id}`}
    >
      <Icon
        name={link.kind === 'album' ? 'disc' : 'user'}
        size={20}
        color={theme.colors.textMuted}
      />
      <View style={styles.text}>
        <Text>{label}</Text>
        <Text variant="caption" muted numberOfLines={1}>
          {link.name}
        </Text>
      </View>
    </Pressable>
  );
}

const styles = StyleSheet.create((theme, rt) => ({
  // Sized by its content: the sheet fits itself to it.
  sheet: {
    backgroundColor: theme.colors.background,
    paddingTop: theme.spacing.lg,
    paddingBottom: theme.spacing.lg + rt.insets.bottom,
  },
  track: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingHorizontal: theme.spacing.lg,
    paddingBottom: theme.spacing.md,
    marginBottom: theme.spacing.xs,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: theme.colors.border,
  },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.lg,
    paddingHorizontal: theme.spacing.lg,
    paddingVertical: theme.spacing.md,
  },
  pressed: { opacity: 0.6 },
  text: { flex: 1, gap: 2 },
}));
