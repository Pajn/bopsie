import { useRouter } from 'expo-router';
import { Pressable, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { Artwork } from '@/features/library/Artwork';
import { Icon } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { nowPlaying } from './nowPlaying';
import { usePlaybackActions, useQueue } from './usePlayback';

/** What the phone is playing, above the tab bar. Opens the full player. */
export function MiniPlayer() {
  const router = useRouter();
  const queue = useQueue().data;
  const playback = usePlaybackActions();
  const current = queue ? nowPlaying(queue) : null;
  if (!queue || !current) return null;
  const playing = queue.state === 'playing';

  return (
    <Pressable
      onPress={() => router.push('/player')}
      style={styles.bar}
      accessibilityRole="button"
      accessibilityLabel={`Now playing: ${current.title}`}
      testID="mini-player"
    >
      <Artwork item={current.artwork} size={40} imageSize={80} />
      <View style={styles.text}>
        <Text numberOfLines={1} testID="mini-player-title">
          {current.title}
        </Text>
        {current.subtitle ? (
          <Text variant="caption" muted numberOfLines={1}>
            {current.subtitle}
          </Text>
        ) : null}
      </View>
      <Pressable
        onPress={() => playback.command('play_pause')}
        hitSlop={12}
        accessibilityRole="button"
        accessibilityLabel={playing ? 'Pause' : 'Play'}
        testID="mini-player-toggle"
      >
        <Icon name={playing ? 'pause' : 'play'} size={26} />
      </Pressable>
    </Pressable>
  );
}

const styles = StyleSheet.create((theme) => ({
  bar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingHorizontal: theme.spacing.lg,
    paddingVertical: theme.spacing.sm,
    backgroundColor: theme.colors.surface,
    borderTopColor: theme.colors.border,
    borderTopWidth: StyleSheet.hairlineWidth,
  },
  text: { flex: 1, gap: 2 },
}));
