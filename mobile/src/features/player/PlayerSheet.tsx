import { useSegments } from 'expo-router';
import { useEffect } from 'react';
import { BackHandler, useWindowDimensions } from 'react-native';
import Animated, { interpolate, useAnimatedStyle } from 'react-native-reanimated';
import { StyleSheet } from 'react-native-unistyles';
import { MiniPlayer } from './MiniPlayer';
import { nowPlaying } from './nowPlaying';
import { PlayerBody } from './PlayerBody';
import {
  collapsePlayer,
  expansion,
  useDockHeight,
  usePlayerOpen,
  useTabBarHeight,
} from './sheetStore';
import { useQueue } from './usePlayback';

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
  const loaded = queue ? nowPlaying(queue) !== null : false;
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
  const covered = !segments.includes('(tabs)') && !OVER_PLAYER.has(segments.at(-1) ?? '');
  useEffect(() => {
    if (covered || !loaded) collapsePlayer();
  }, [covered, loaded]);

  // Android's back button is the way down from a screen, and the player stands in for one.
  useEffect(() => {
    if (!open) return;
    const subscription = BackHandler.addEventListener('hardwareBackPress', () => {
      collapsePlayer();
      return true;
    });
    return () => subscription.remove();
  }, [open]);

  // The dock is where the sheet is laid out, and the animation only carries it away from
  // there: a resting place held in the animated style would reach the screen a frame behind
  // the layout around it whenever that layout settles, at launch most of all.
  const sheetStyle = useAnimatedStyle(() => ({
    transform: [{ translateY: -expansion.value * travel }],
  }));
  // What is behind has to be covered as soon as the sheet leaves the dock.
  const backdropStyle = useAnimatedStyle(() => ({
    opacity: Math.min(1, expansion.value * 12),
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
    </Animated.View>
  );
}

const styles = StyleSheet.create((theme) => ({
  sheet: { position: 'absolute', left: 0, right: 0 },
  backdrop: { ...StyleSheet.absoluteFillObject, backgroundColor: theme.colors.background },
  body: { ...StyleSheet.absoluteFillObject },
  /** The mini player the sheet rests as, held at its top edge. */
  dock: { position: 'absolute', top: 0, left: 0, right: 0 },
}));
