import { useRouter } from 'expo-router';
import { Pressable, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { Artwork } from '@/features/library/Artwork';
import { useOutputs } from '@/features/outputs/useOutputs';
import { Icon } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { nowPlaying } from './nowPlaying';
import { usePlaybackActions, useQueue } from './usePlayback';

/**
 * What the active group is playing, above the tab bar. Opens the full player,
 * or the output sheet from its speaker button. Names the group unless it is
 * the phone.
 */
export function MiniPlayer() {
  const router = useRouter();
  const queue = useQueue().data;
  const playback = usePlaybackActions();
  const { queueId: phoneId } = useConnection();
  const active = useOutputs()?.active;
  const current = queue ? nowPlaying(queue) : null;
  if (!queue || !current) return null;
  const playing = queue.state === 'playing';
  const where = active && active.id !== phoneId ? `on ${active.name}` : null;

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
        {where || current.subtitle ? (
          <Text variant="caption" muted numberOfLines={1} testID="mini-player-subtitle">
            {where ?? current.subtitle}
          </Text>
        ) : null}
      </View>
      <Pressable
        onPress={() => router.push('/outputs')}
        hitSlop={12}
        accessibilityRole="button"
        accessibilityLabel="Choose where to play"
        testID="mini-player-outputs"
      >
        <Icon name="speaker" size={22} />
      </Pressable>
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
