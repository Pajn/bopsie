import { useSegments } from 'expo-router';
import { useEffect, useRef } from 'react';
import { BackHandler, useWindowDimensions, View, type ViewInstance } from 'react-native';
import Animated, {
  interpolate,
  interpolateColor,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
} from 'react-native-reanimated';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { CoverPlacesProvider, FlyingCover, useCoverPlaces } from './FlyingCover';
import { MiniPlayer } from './MiniPlayer';
import { NOTHING_PLAYING, nowPlaying } from './nowPlaying';
import { PlayerBody } from './PlayerBody';
import {
  collapsePlayer,
  expansion,
  useDockHeight,
  usePlayerOpen,
  useTabBarHeight,
} from './sheetStore';
import { useQueue } from './usePlayback';
import { PlayerTintProvider, useCoverTint } from './usePlayerTint';

/** Screens that open over the player as sheets, which leave it up beneath them. */
const OVER_PLAYER = new Set(['player-menu']);

/**
 * The player, from the mini player above the tab bar to the whole screen, as one view that a
 * finger moves between the two.
 *
 * It is not a screen: a pushed screen arrives on its own animation, which a drag cannot be
 * part of. It sits over the tabs, and the mini player it rests as is the foot of the same
 * view it grows out of.
 */
export function PlayerSheet() {
  const { height } = useWindowDimensions();
  const queue = useQueue().data;
  const current = queue ? nowPlaying(queue) : null;
  const frame = useRef<ViewInstance>(null);
  const covers = useCoverPlaces(frame);
  const tint = useCoverTint(current?.artwork ?? null);
  const { theme } = useUnistyles();
  const background = tint?.background ?? theme.colors.background;
  // The background cross-fades from the last cover's tint to the next.
  const fromColor = useSharedValue(background);
  const toColor = useSharedValue(background);
  const fade = useSharedValue(1);
  useEffect(() => {
    if (background === toColor.value) return;
    fromColor.value = interpolateColor(fade.value, [0, 1], [fromColor.value, toColor.value]);
    toColor.value = background;
    fade.value = 0;
    fade.value = withTiming(1, { duration: 450 });
  }, [background, fromColor, toColor, fade]);
  const open = usePlayerOpen();
  const dockHeight = useDockHeight();
  const tabBarHeight = useTabBarHeight();

  /** Where the sheet's top edge rests while docked, and so how far it climbs to cover all. */
  const restTop = Math.max(0, height - tabBarHeight);
  const travel = restTop;

  // The sheet is drawn inside the tabs, so a screen that covers them leaves it up with nothing
  // to show for it, and still up when that screen closes. It goes back to its dock instead,
  // unless what opened is a sheet over the player itself. Typed routes narrow the segments to
  // routes that exist, so they are widened to ask about a group by name.
  const segments: readonly string[] = useSegments();
  const onTop = segments.includes('(tabs)');
  const covered = !onTop && !OVER_PLAYER.has(segments.at(-1) ?? '');
  useEffect(() => {
    if (covered) collapsePlayer();
  }, [covered]);

  // Android's back button is the way down from a screen, and the player stands in for one,
  // while nothing is open over it: a sheet over the player, such as its menu, closes first.
  useEffect(() => {
    if (!open || !onTop) return;
    const subscription = BackHandler.addEventListener('hardwareBackPress', () => {
      collapsePlayer();
      return true;
    });
    return () => subscription.remove();
  }, [open, onTop]);

  // The dock is where the sheet is laid out, and the animation only carries it away from
  // there: a resting place held in the animated style would reach the screen a frame behind
  // the layout around it whenever that layout settles, at launch most of all.
  const sheetStyle = useAnimatedStyle(() => ({
    transform: [{ translateY: -expansion.value * travel }],
  }));
  // What is behind has to be covered as soon as the sheet leaves the dock.
  const backdropStyle = useAnimatedStyle(() => ({
    opacity: Math.min(1, expansion.value * 12),
    backgroundColor: interpolateColor(fade.value, [0, 1], [fromColor.value, toColor.value]),
  }));
  const bodyStyle = useAnimatedStyle(() => ({
    opacity: interpolate(expansion.value, [0.1, 0.55], [0, 1], 'clamp'),
  }));
  const dockStyle = useAnimatedStyle(() => ({
    opacity: interpolate(expansion.value, [0, 0.18], [1, 0], 'clamp'),
  }));

  if (tabBarHeight === 0) return null;
  return (
    <Animated.View
      style={[styles.sheet, { height, top: restTop }, sheetStyle]}
      pointerEvents="box-none"
      testID="player-sheet"
    >
      <View ref={frame} style={styles.frame} pointerEvents="box-none">
        <CoverPlacesProvider value={covers.value}>
          <PlayerTintProvider tint={tint}>
            <Animated.View style={[styles.backdrop, backdropStyle]} pointerEvents="none" />
            <Animated.View
              style={[styles.body, bodyStyle]}
              pointerEvents={open ? 'auto' : 'none'}
              accessibilityElementsHidden={!open}
              importantForAccessibility={open ? 'auto' : 'no-hide-descendants'}
            >
              <PlayerBody open={open} travel={travel} />
            </Animated.View>
            <Animated.View
              style={[styles.dock, { height: dockHeight || undefined }, dockStyle]}
              pointerEvents={open ? 'none' : 'box-none'}
            >
              <MiniPlayer travel={travel} />
            </Animated.View>
            {covers.places.mini && covers.places.full ? (
              <FlyingCover
                artwork={current?.artwork ?? NOTHING_PLAYING.artwork}
                mini={covers.places.mini}
                full={covers.places.full}
              />
            ) : null}
          </PlayerTintProvider>
        </CoverPlacesProvider>
      </View>
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  sheet: { position: 'absolute', left: 0, right: 0 },
  /** What the artwork's places are measured against. */
  frame: { ...StyleSheet.absoluteFillObject },
  backdrop: { ...StyleSheet.absoluteFillObject },
  body: { ...StyleSheet.absoluteFillObject },
  /** The mini player the sheet rests as, held at its top edge. */
  dock: { position: 'absolute', top: 0, left: 0, right: 0 },
});
