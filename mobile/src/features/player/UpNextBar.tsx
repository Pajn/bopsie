import { useRouter } from 'expo-router';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { Artwork } from '@/features/library/Artwork';
import { Icon } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { describeItem, type HeardQueue } from './nowPlaying';

/**
 * The foot of the full player: what plays next, or what the group plays from in place of
 * its queue. Opens the queue.
 */
export function UpNextBar({ queue }: { queue: HeardQueue }) {
  const router = useRouter();
  const { theme } = useUnistyles();
  const next = queue.next_item && !queue.source ? describeItem(queue.next_item) : null;
  const label = queue.source ? 'Playing from' : 'Up next';
  const title = queue.source ? queue.source.name : (next?.title ?? 'Nothing else is queued');
  // The album would crowd out the artists on one line; the queue has it.
  const artists = next
    ? queue.next_item?.media_item?.artists.map((artist) => artist.name).join(', ')
    : undefined;

  return (
    <Pressable
      onPress={() => router.push('/queue')}
      style={({ pressed }) => [styles.bar, pressed && styles.pressed]}
      accessibilityRole="button"
      accessibilityLabel={`${label}: ${title}. Open the queue`}
      testID="player-queue"
    >
      {next ? (
        <Artwork item={next.artwork} size={36} imageSize={80} />
      ) : (
        <View style={styles.glyph}>
          <Icon name={queue.source ? 'speaker' : 'list'} size={18} color={theme.colors.textMuted} />
        </View>
      )}
      <View style={styles.text}>
        <Text variant="caption" muted style={styles.label}>
          {label}
        </Text>
        <Text numberOfLines={1} style={next ? null : styles.quiet} testID="player-up-next">
          {title}
          {artists ? <Text style={styles.quiet}>{` · ${artists}`}</Text> : null}
        </Text>
      </View>
      <Icon name="chevron-up" size={18} color={theme.colors.textMuted} />
    </Pressable>
  );
}

const styles = StyleSheet.create((theme) => ({
  bar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    padding: theme.spacing.sm,
    paddingRight: theme.spacing.md,
    borderRadius: theme.radius.md,
    backgroundColor: theme.colors.surfaceVariant,
  },
  pressed: { opacity: 0.6 },
  glyph: { width: 36, height: 36, alignItems: 'center', justifyContent: 'center' },
  text: { flex: 1 },
  label: { textTransform: 'uppercase', letterSpacing: 0.6 },
  quiet: { color: theme.colors.textMuted },
}));
