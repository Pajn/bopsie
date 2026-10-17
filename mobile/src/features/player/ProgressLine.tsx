import { useEffect } from 'react';
import { View } from 'react-native';
import Animated, {
  cancelAnimation,
  Easing,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
} from 'react-native-reanimated';
import { StyleSheet } from 'react-native-unistyles';
import { advancing, positionAt } from '@/player/progress';
import type { HeardQueue } from './nowPlaying';

/**
 * A thin line along the top of the mini player, as far across as the track has played. It
 * takes the server's position whenever that changes and runs on to the end from there while
 * playing, by transform, so it moves smoothly without the mini player drawing again.
 */
export function ProgressLine({ queue }: { queue: HeardQueue }) {
  const progress = useSharedValue(0);
  const duration = queue.current_item?.duration ?? 0;
  const playing = advancing(queue);
  const { elapsed_time: elapsed, elapsed_time_last_updated: measuredAt } = queue;
  const rate = queue.playback_speed;

  useEffect(() => {
    cancelAnimation(progress);
    if (!duration) {
      progress.value = 0;
      return;
    }
    const position = positionAt({ elapsed, measuredAt, playing, duration, rate }, Date.now());
    progress.value = Math.min(1, position / duration);
    if (playing && position < duration) {
      progress.value = withTiming(1, {
        duration: ((duration - position) / rate) * 1000,
        easing: Easing.linear,
      });
    }
  }, [progress, duration, playing, elapsed, measuredAt, rate]);

  const style = useAnimatedStyle(() => ({ transform: [{ scaleX: progress.value }] }));
  if (!duration) return null;
  return (
    <View style={styles.track} pointerEvents="none">
      <Animated.View style={[styles.fill, style]} />
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  track: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    height: 2,
    backgroundColor: theme.colors.border,
  },
  fill: { flex: 1, backgroundColor: theme.colors.accent, transformOrigin: 'left' },
}));
