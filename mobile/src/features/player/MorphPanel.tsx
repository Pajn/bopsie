import { type ReactNode, useCallback, useEffect, useState } from 'react';
import { BackHandler, Pressable, View } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, {
  cancelAnimation,
  Easing,
  interpolate,
  interpolateColor,
  useAnimatedReaction,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
} from 'react-native-reanimated';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { scheduleOnRN } from 'react-native-worklets';
import { Icon, type IconName } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { dragged, settlesOpen } from './settle';
import { panelCoverage } from './sheetStore';

/** Where something sits in the player. */
export interface Frame {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** How the panel looks while it is closed: the shape of what it grows from. */
export interface ClosedLook {
  radius: number;
  paddingLeft: number;
  paddingRight: number;
  /** Between what it shows and the chevron. */
  gap: number;
  chevron: { size: number; color: string };
}

const HEADER_HEIGHT = 56;
const OPEN_CHEVRON = 24;
// Material's emphasized easing: quick to leave, slow to settle.
const EASING = Easing.bezier(0.2, 0, 0, 1);

/**
 * Something in the full player that grows into a panel over all of it: from where it sits,
 * `from`, to the player's size. Its content gives way to a header with `title` while the
 * panel's own content, `children`, fades in below, and the chevron turns; the header, or
 * back, close it again. Dragging it towards open, or the header towards closed, moves it with
 * the finger. `children` are made once the player shows, and kept.
 *
 * @param closed what it shows while closed, beside the chevron.
 * @param chevron the chevron's direction while closed; it turns to point the other way.
 */
export function MorphPanel({
  from,
  player,
  look,
  closed,
  chevron,
  title,
  openLabel,
  testID,
  ready,
  children,
}: {
  from: Frame;
  player: { width: number; height: number };
  look: ClosedLook;
  closed: ReactNode;
  chevron: Extract<IconName, 'chevron-up' | 'chevron-down'>;
  title: string;
  /** What opening it does, for screen readers, such as "Up next: Teardrop. Open the queue". */
  openLabel: string;
  testID: string;
  /** Whether the player shows, and so the panel could open: its content is made then. */
  ready: boolean;
  children: ReactNode;
}) {
  const { theme, rt } = useUnistyles();
  const [open, setOpen] = useState(false);
  const [loaded, setLoaded] = useState(false);
  // Above the player's other panels from opening until it has closed again.
  const [raised, setRaised] = useState(false);
  const progress = useSharedValue(0);
  const topInset = rt.insets.top;
  const closedColor = theme.colors.surfaceVariant;
  const openColor = theme.colors.background;
  const openPadding = theme.spacing.lg;
  // How far the panel's top moves between closed and open, which a drag covers.
  const travel = Math.max(from.y, 1);
  const dragStart = useSharedValue(0);

  const begin = useCallback(() => {
    setLoaded(true);
    setRaised(true);
  }, []);
  const toggle = useCallback(
    (next: boolean) => {
      setOpen(next);
      if (next) begin();
      progress.value = withTiming(
        next ? 1 : 0,
        { duration: next ? 380 : 300, easing: EASING },
        (finished) => {
          if (finished && !next) scheduleOnRN(setRaised, false);
        },
      );
    },
    [progress, begin],
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
      scheduleOnRN(begin);
    })
    .onUpdate((event) => {
      progress.value = dragged(dragStart.value, event.translationY, travel);
    })
    .onEnd((event) => {
      scheduleOnRN(toggle, settlesOpen(progress.value, event.velocityY, travel));
    });

  // Only the panel moving reports, so a closed one leaves the other's coverage alone.
  useAnimatedReaction(
    () => progress.value,
    (value, previous) => {
      if (previous !== null && value !== previous) panelCoverage.value = value;
    },
  );

  const panelStyle = useAnimatedStyle(() => {
    const p = progress.value;
    return {
      left: interpolate(p, [0, 1], [from.x, 0]),
      top: interpolate(p, [0, 1], [from.y, 0]),
      width: interpolate(p, [0, 1], [from.width, player.width]),
      height: interpolate(p, [0, 1], [from.height, player.height]),
      borderRadius: interpolate(p, [0, 1], [look.radius, 0]),
      backgroundColor: interpolateColor(p, [0, 1], [closedColor, openColor]),
    };
  });
  // The status bar's space above the header, which a drag can start in too.
  const topStyle = useAnimatedStyle(() => ({
    paddingTop: interpolate(progress.value, [0, 1], [0, topInset]),
  }));
  const headerStyle = useAnimatedStyle(() => ({
    height: interpolate(progress.value, [0, 1], [from.height, HEADER_HEIGHT]),
    paddingLeft: interpolate(progress.value, [0, 1], [look.paddingLeft, openPadding]),
    paddingRight: interpolate(progress.value, [0, 1], [look.paddingRight, openPadding]),
  }));
  const closedStyle = useAnimatedStyle(() => ({
    opacity: interpolate(progress.value, [0, 0.4], [1, 0], 'clamp'),
  }));
  const titleStyle = useAnimatedStyle(() => ({
    opacity: interpolate(progress.value, [0.5, 1], [0, 1], 'clamp'),
  }));
  // The chevron grows, its place with it, and takes the header's colour as it turns.
  const chevronStyle = useAnimatedStyle(() => {
    const p = progress.value;
    const size = interpolate(p, [0, 1], [look.chevron.size, OPEN_CHEVRON]);
    return { width: size, height: size, transform: [{ rotate: `${p * 180}deg` }] };
  });
  const scale = (p: number) => {
    'worklet';
    return interpolate(p, [0, 1], [look.chevron.size, OPEN_CHEVRON]) / OPEN_CHEVRON;
  };
  const closedChevronStyle = useAnimatedStyle(() => ({
    opacity: 1 - progress.value,
    transform: [{ scale: scale(progress.value) }],
  }));
  const openChevronStyle = useAnimatedStyle(() => ({
    opacity: progress.value,
    transform: [{ scale: scale(progress.value) }],
  }));
  // The content keeps its open size and its place on the screen while the panel grows over
  // it, so nothing in it is laid out again on each frame; the panel only uncovers it.
  const contentTop = topInset + HEADER_HEIGHT;
  const bodyStyle = useAnimatedStyle(() => {
    const p = progress.value;
    return {
      opacity: interpolate(p, [0.5, 1], [0, 1], 'clamp'),
      left: -interpolate(p, [0, 1], [from.x, 0]),
      top: contentTop - interpolate(p, [0, 1], [from.y, 0]),
    };
  });

  // Made a moment after the player shows rather than when the panel opens, so opening it has
  // nothing to make first.
  useEffect(() => {
    if (!ready || loaded) return;
    const timer = setTimeout(() => setLoaded(true), 500);
    return () => clearTimeout(timer);
  }, [ready, loaded]);

  return (
    <Animated.View
      style={[styles.panel, raised && styles.raised, panelStyle]}
      accessibilityViewIsModal={open}
      testID={testID}
    >
      <GestureDetector gesture={drag}>
        <Animated.View style={topStyle}>
          <Pressable
            onPress={() => toggle(!open)}
            accessibilityRole="button"
            accessibilityLabel={open ? `Close ${title}` : openLabel}
            accessibilityState={{ expanded: open }}
            testID={`${testID}-toggle`}
          >
            <Animated.View style={[styles.header, { gap: look.gap }, headerStyle]}>
              <View style={styles.content}>
                <Animated.View style={[styles.fill, closedStyle]}>{closed}</Animated.View>
                <Animated.View style={[styles.fill, titleStyle]} pointerEvents="none">
                  <Text variant="heading">{title}</Text>
                </Animated.View>
              </View>
              <Animated.View style={[styles.chevron, chevronStyle]}>
                <Animated.View style={[styles.fill, styles.centre, closedChevronStyle]}>
                  <Icon name={chevron} size={OPEN_CHEVRON} color={look.chevron.color} />
                </Animated.View>
                <Animated.View style={[styles.fill, styles.centre, openChevronStyle]}>
                  <Icon name={chevron} size={OPEN_CHEVRON} color={theme.colors.textMuted} />
                </Animated.View>
              </Animated.View>
            </Animated.View>
          </Pressable>
        </Animated.View>
      </GestureDetector>
      <Animated.View
        style={[
          styles.body,
          { width: player.width, height: player.height - contentTop },
          bodyStyle,
        ]}
        pointerEvents={open ? 'auto' : 'none'}
      >
        {loaded ? children : null}
      </Animated.View>
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  // Above the seek bar, which raises itself, and which its flattened parents leave beside the panel.
  panel: { position: 'absolute', overflow: 'hidden', zIndex: 2 },
  raised: { zIndex: 3 },
  header: { flexDirection: 'row', alignItems: 'center' },
  // What the panel shows closed, and its title open, in the same place.
  content: { flex: 1, alignSelf: 'stretch' },
  fill: { ...StyleSheet.absoluteFillObject, justifyContent: 'center' },
  // Drawn at its open size and scaled, overflowing its place while smaller.
  centre: { alignItems: 'center', overflow: 'visible' },
  chevron: { overflow: 'visible' },
  body: { position: 'absolute' },
});
