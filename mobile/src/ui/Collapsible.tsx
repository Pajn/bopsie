import { type ReactNode, useEffect, useEffectEvent } from 'react';
import { View } from 'react-native';
import Animated, {
  Easing,
  type SharedValue,
  useAnimatedStyle,
  useSharedValue,
  withTiming,
} from 'react-native-reanimated';
import { StyleSheet } from 'react-native-unistyles';
import { scheduleOnRN } from 'react-native-worklets';

// Material's emphasized easing: quick to leave, slow to settle.
const EASING = Easing.bezier(0.2, 0, 0, 1);
const DURATION = 280;

/**
 * How open something is, from 0 closed to 1 open, moving to `open` whenever it changes, so
 * what opens and what shows it, such as a turning chevron, move together.
 */
export function useOpenProgress(open: boolean): SharedValue<number> {
  const progress = useSharedValue(open ? 1 : 0);
  useEffect(() => {
    progress.value = withTiming(open ? 1 : 0, { duration: DURATION, easing: EASING });
  }, [open, progress]);
  return progress;
}

/**
 * Content that unfolds to its full height as `progress` goes to 1, and folds away as it goes
 * to 0, pushing what follows it along. It stays mounted while folded, hidden from touch and
 * from screen readers.
 */
export function Collapsible({
  progress,
  open,
  children,
}: {
  progress: SharedValue<number>;
  open: boolean;
  children: ReactNode;
}) {
  const height = useSharedValue(0);
  const style = useAnimatedStyle(() => ({
    height: height.value * progress.value,
    opacity: progress.value,
  }));
  return (
    <Animated.View
      style={[styles.clip, style]}
      pointerEvents={open ? 'auto' : 'none'}
      importantForAccessibility={open ? 'auto' : 'no-hide-descendants'}
      accessibilityElementsHidden={!open}
    >
      {/* Laid out at its own height whatever the clip's, which is what it unfolds to. */}
      <View
        style={styles.content}
        onLayout={({ nativeEvent }) => {
          height.value = nativeEvent.layout.height;
        }}
      >
        {children}
      </View>
    </Animated.View>
  );
}

/**
 * An item of a list that unfolds into it as it arrives and folds away as it leaves, so what
 * follows it moves with it; see `usePresence`. Once folded away it calls `onLeft`.
 */
export function Fold({
  present,
  arriving,
  onLeft,
  children,
}: {
  present: boolean;
  arriving: boolean;
  onLeft: () => void;
  children: ReactNode;
}) {
  const progress = useSharedValue(present && !arriving ? 1 : 0);
  const left = useEffectEvent(onLeft);
  useEffect(() => {
    const done = () => left();
    progress.value = withTiming(
      present ? 1 : 0,
      { duration: DURATION, easing: EASING },
      (finished) => {
        if (finished && !present) scheduleOnRN(done);
      },
    );
  }, [present, progress]);
  return (
    <Collapsible progress={progress} open={present}>
      {children}
    </Collapsible>
  );
}

const styles = StyleSheet.create({
  clip: { overflow: 'hidden' },
  content: { position: 'absolute', left: 0, right: 0, top: 0 },
});
