import { useEffect } from 'react';
import { View } from 'react-native';
import Animated, {
  cancelAnimation,
  Easing,
  useAnimatedStyle,
  useSharedValue,
  withRepeat,
  withTiming,
} from 'react-native-reanimated';
import { StyleSheet } from 'react-native-unistyles';

/**
 * Rows standing in for a list while it loads, shaped like the rows that will replace them, so
 * the list fills in rather than jumping in under a spinner. They pulse, together, by opacity.
 */
export function SkeletonRows({
  count = 8,
  round,
  artwork = 48,
}: {
  count?: number;
  round?: boolean;
  artwork?: number;
}) {
  const pulse = useSharedValue(1);
  useEffect(() => {
    pulse.value = withRepeat(
      withTiming(0.45, { duration: 750, easing: Easing.inOut(Easing.quad) }),
      -1,
      true,
    );
    return () => cancelAnimation(pulse);
  }, [pulse]);
  const style = useAnimatedStyle(() => ({ opacity: pulse.value }));
  return (
    <Animated.View style={style} accessibilityLabel="Loading" testID="skeleton">
      {Array.from({ length: count }, (_, index) => `row-${index}`).map((key, index) => (
        <View key={key} style={styles.row}>
          <View
            style={[
              styles.block,
              { width: artwork, height: artwork, borderRadius: round ? artwork / 2 : 6 },
            ]}
          />
          <View style={styles.text}>
            <View style={[styles.block, styles.line, { width: `${70 - ((index * 17) % 30)}%` }]} />
            <View style={[styles.block, styles.line, styles.short]} />
          </View>
        </View>
      ))}
    </Animated.View>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingVertical: theme.spacing.sm,
  },
  block: { backgroundColor: theme.colors.surfaceVariant },
  text: { flex: 1, gap: theme.spacing.xs + 2 },
  line: { height: 12, borderRadius: 6 },
  short: { width: '35%' },
}));
