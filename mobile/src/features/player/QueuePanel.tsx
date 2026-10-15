import { useCallback, useEffect, useState } from 'react';
import { BackHandler, Platform, Pressable, View } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, {
  cancelAnimation,
  Easing,
  interpolate,
  interpolateColor,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
} from 'react-native-reanimated';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { scheduleOnRN } from 'react-native-worklets';
import { Artwork } from '@/features/library/Artwork';
import { QueueList } from '@/features/queue/QueueList';
import { Icon } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { describeItem, type HeardQueue } from './nowPlaying';
import { settlesOpen } from './settle';

/** The height of the bar the panel rises from, which the player keeps free for it. */
export const QUEUE_BAR_HEIGHT = 56;

export interface Frame {
  x: number;
  y: number;
  width: number;
  height: number;
}

const ARTWORK = 36;
// Material's emphasized easing: quick to leave, slow to settle.
const EASING = Easing.bezier(0.2, 0, 0, 1);

/**
 * The foot of the full player: what plays next, or what the group plays from in place of
 * its queue. Opened, it grows from there to fill the player with the queue, its bar
 * becoming the queue's header and its chevron turning down; the header, or back, close it
 * again. Dragging the bar up, or the header down, moves it with the finger.
 *
 * @param bar where the bar sits in the player, and `player` the player's size.
 */
export function QueuePanel({
  queue,
  bar,
  player,
}: {
  queue: HeardQueue;
  bar: Frame;
  player: { width: number; height: number };
}) {
  const { theme, rt } = useUnistyles();
  const [open, setOpen] = useState(false);
  // The queue loads the first time it opens, then stays, so closing never shows it empty.
  const [loaded, setLoaded] = useState(false);
  const progress = useSharedValue(0);
  // iOS presents the player as a sheet below the status bar; Android draws it edge to edge.
  const topInset = Platform.OS === 'ios' ? 0 : rt.insets.top;
  const closedColor = theme.colors.surfaceVariant;
  const openColor = theme.colors.background;
  const closedRadius = theme.radius.md;
  const closedPadding = theme.spacing.sm;
  const openPadding = theme.spacing.lg;
  const gap = theme.spacing.md;

  // How far the panel's top moves between closed and open, which a drag covers.
  const travel = Math.max(bar.y, 1);
  const dragStart = useSharedValue(0);

  const toggle = useCallback(
    (next: boolean) => {
      setOpen(next);
      if (next) setLoaded(true);
      progress.value = withTiming(next ? 1 : 0, { duration: next ? 380 : 300, easing: EASING });
    },
    [progress],
  );

  useEffect(() => {
    if (!open) return;
    const subscription = BackHandler.addEventListener('hardwareBackPress', () => {
      toggle(false);
      return true;
    });
    return () => subscription.remove();
  }, [open, toggle]);

  const drag = Gesture.Pan()
    // Vertical only, and only once it is clearly a drag, so a tap still toggles.
    .activeOffsetY([-10, 10])
    .failOffsetX([-20, 20])
    .onStart(() => {
      cancelAnimation(progress);
      dragStart.value = progress.value;
      scheduleOnRN(setLoaded, true);
    })
    .onUpdate((event) => {
      const next = dragStart.value - event.translationY / travel;
      progress.value = Math.min(1, Math.max(0, next));
    })
    .onEnd((event) => {
      scheduleOnRN(toggle, settlesOpen(progress.value, event.velocityY, travel));
    });

  const panelStyle = useAnimatedStyle(() => {
    const p = progress.value;
    return {
      left: interpolate(p, [0, 1], [bar.x, 0]),
      top: interpolate(p, [0, 1], [bar.y, 0]),
      width: interpolate(p, [0, 1], [bar.width, player.width]),
      height: interpolate(p, [0, 1], [bar.height, player.height]),
      borderRadius: interpolate(p, [0, 1], [closedRadius, 0]),
      backgroundColor: interpolateColor(p, [0, 1], [closedColor, openColor]),
    };
  });
  // The status bar's space above the header, which a drag can start in too.
  const topStyle = useAnimatedStyle(() => ({
    paddingTop: interpolate(progress.value, [0, 1], [0, topInset]),
  }));
  const headerStyle = useAnimatedStyle(() => ({
    paddingLeft: interpolate(progress.value, [0, 1], [closedPadding, openPadding]),
  }));
  // The artwork gives up its place as the bar becomes a header.
  const artworkStyle = useAnimatedStyle(() => ({
    opacity: interpolate(progress.value, [0, 0.4], [1, 0], 'clamp'),
    width: interpolate(progress.value, [0, 1], [ARTWORK, 0]),
    marginRight: interpolate(progress.value, [0, 1], [gap, 0]),
  }));
  const barTextStyle = useAnimatedStyle(() => ({
    opacity: interpolate(progress.value, [0, 0.4], [1, 0], 'clamp'),
  }));
  const titleStyle = useAnimatedStyle(() => ({
    opacity: interpolate(progress.value, [0.5, 1], [0, 1], 'clamp'),
  }));
  const chevronStyle = useAnimatedStyle(() => ({
    transform: [{ rotate: `${progress.value * 180}deg` }],
  }));
  const bodyStyle = useAnimatedStyle(() => ({
    opacity: interpolate(progress.value, [0.5, 1], [0, 1], 'clamp'),
  }));

  const next = queue.next_item && !queue.source ? describeItem(queue.next_item) : null;
  const label = queue.source ? 'Playing from' : 'Up next';
  const title = queue.source ? queue.source.name : (next?.title ?? 'Nothing else is queued');
  // The album would crowd out the artists on one line; the queue has it.
  const artists = next
    ? queue.next_item?.media_item?.artists.map((artist) => artist.name).join(', ')
    : undefined;

  return (
    <Animated.View
      style={[styles.panel, panelStyle]}
      accessibilityViewIsModal={open}
      testID="player-queue-panel"
    >
      <GestureDetector gesture={drag}>
        <Animated.View style={topStyle}>
          <Pressable
            onPress={() => toggle(!open)}
            accessibilityRole="button"
            accessibilityLabel={open ? 'Close the queue' : `${label}: ${title}. Open the queue`}
            accessibilityState={{ expanded: open }}
            testID="player-queue"
          >
            <Animated.View style={[styles.header, headerStyle]}>
              <Animated.View style={[styles.artwork, artworkStyle]}>
                {next ? (
                  <Artwork item={next.artwork} size={ARTWORK} imageSize={80} />
                ) : (
                  <Icon
                    name={queue.source ? 'speaker' : 'list'}
                    size={18}
                    color={theme.colors.textMuted}
                  />
                )}
              </Animated.View>
              <View style={styles.text}>
                <Animated.View style={barTextStyle}>
                  <Text variant="caption" muted style={styles.label}>
                    {label}
                  </Text>
                  <Text
                    numberOfLines={1}
                    style={next ? null : styles.quiet}
                    testID="player-up-next"
                  >
                    {title}
                    {artists ? <Text style={styles.quiet}>{` · ${artists}`}</Text> : null}
                  </Text>
                </Animated.View>
                <Animated.View style={[styles.title, titleStyle]} pointerEvents="none">
                  <Text variant="heading">Queue</Text>
                </Animated.View>
              </View>
              <Animated.View style={chevronStyle}>
                <Icon name="chevron-up" size={open ? 24 : 18} color={theme.colors.textMuted} />
              </Animated.View>
            </Animated.View>
          </Pressable>
        </Animated.View>
      </GestureDetector>
      <Animated.View style={[styles.body, bodyStyle]} pointerEvents={open ? 'auto' : 'none'}>
        {loaded ? <QueueList /> : null}
      </Animated.View>
    </Animated.View>
  );
}

const styles = StyleSheet.create((theme) => ({
  // Above the seek bar, which raises itself, and which its flattened parents leave beside the panel.
  panel: { position: 'absolute', overflow: 'hidden', zIndex: 2 },
  header: {
    height: QUEUE_BAR_HEIGHT,
    flexDirection: 'row',
    alignItems: 'center',
    paddingRight: theme.spacing.md,
  },
  artwork: { height: ARTWORK, alignItems: 'center', justifyContent: 'center', overflow: 'hidden' },
  text: { flex: 1, justifyContent: 'center' },
  label: { textTransform: 'uppercase', letterSpacing: 0.6 },
  quiet: { color: theme.colors.textMuted },
  title: { ...StyleSheet.absoluteFillObject, justifyContent: 'center' },
  body: { flex: 1 },
}));
