import { getQueue } from '@bopsie/client/api/endpoints/players';
import type { PlayerQueue } from '@bopsie/client/api/schemas/player';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useEffect } from 'react';
import { View } from 'react-native';
import Animated, {
  cancelAnimation,
  Easing,
  type SharedValue,
  useAnimatedStyle,
  useSharedValue,
  withDelay,
  withRepeat,
  withTiming,
} from 'react-native-reanimated';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { useActiveGroup } from '@/features/outputs/ActiveGroupProvider';

/** What the active group plays from its queue, and whether it is playing, as `uri playing`. */
const heard = (queue: PlayerQueue | null | undefined) => {
  const uri = queue?.current_item?.media_item?.uri;
  return uri ? `${uri} ${queue.state === 'playing'}` : null;
};

/**
 * Whether `uri` is what the active group plays from its queue: `playing`, `paused`, or null.
 * It reads only the track and the state from the cached queue, so a list of rows re-renders
 * when the track or the state changes rather than each time the position moves. A source, such
 * as Spotify Connect, playing in place of the queue plays none of it.
 */
export function usePlayingState(uri: string): 'playing' | 'paused' | null {
  const { connection, account } = useConnection();
  const { activeId, players } = useActiveGroup();
  const fromQueue = useQuery({
    queryKey: queryKeys.queue(account.id, activeId ?? 'none'),
    queryFn: () => (activeId ? getQueue(connection, activeId) : null),
    enabled: activeId !== null,
    staleTime: Number.POSITIVE_INFINITY,
    select: heard,
  }).data;
  const player = players?.find((candidate) => candidate.player_id === activeId);
  const source = player?.active_source;
  if (!fromQueue || (source && source !== activeId)) return null;
  const [heardUri, playing] = fromQueue.split(' ');
  if (heardUri !== uri) return null;
  return playing === 'true' ? 'playing' : 'paused';
}

const BARS = [0, 1, 2];
const PERIODS = [420, 560, 340];

/**
 * Three bars rising and falling while `playing`, resting low while paused: marks what plays in
 * a list. They move by transform only.
 */
export function PlayingBars({
  playing,
  size = 14,
  color,
}: {
  playing: boolean;
  size?: number;
  color?: string;
}) {
  const { theme } = useUnistyles();
  const fill = color ?? theme.colors.accent;
  const width = Math.max(2, Math.round(size / 5));
  return (
    <View style={[styles.bars, { width: size, height: size }]} accessibilityElementsHidden>
      {BARS.map((index) => (
        <Bar key={index} index={index} playing={playing} width={width} height={size} color={fill} />
      ))}
    </View>
  );
}

function Bar({
  index,
  playing,
  width,
  height,
  color,
}: {
  index: number;
  playing: boolean;
  width: number;
  height: number;
  color: string;
}) {
  const level = useSharedValue(0.3);
  useEffect(() => {
    bounce(level, playing, index);
    return () => cancelAnimation(level);
  }, [level, playing, index]);
  const style = useAnimatedStyle(() => ({ transform: [{ scaleY: level.value }] }));
  return (
    <Animated.View
      style={[
        styles.bar,
        { width, height, backgroundColor: color, borderRadius: width / 2 },
        style,
      ]}
    />
  );
}

function bounce(level: SharedValue<number>, playing: boolean, index: number) {
  cancelAnimation(level);
  if (!playing) {
    level.value = withTiming(0.3, { duration: 200 });
    return;
  }
  const period = PERIODS[index] ?? 400;
  level.value = withDelay(
    index * 90,
    withRepeat(withTiming(1, { duration: period, easing: Easing.inOut(Easing.quad) }), -1, true),
  );
}

const styles = StyleSheet.create({
  bars: { flexDirection: 'row', alignItems: 'flex-end', justifyContent: 'space-between' },
  bar: { transformOrigin: 'bottom' },
});
