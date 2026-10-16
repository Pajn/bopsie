import { useEffect, useRef } from 'react';
import { Animated, type DimensionValue, View } from 'react-native';
import { useUnistyles } from 'react-native-unistyles';

/**
 * A grey block where something is still loading, breathing so it reads as on its way: the
 * shape of what comes, so the page does not jump when it does.
 */
export function Skeleton({
  width = '100%',
  height,
  radius,
  round,
}: {
  width?: DimensionValue;
  height?: DimensionValue;
  radius?: number;
  round?: boolean;
}) {
  const { theme } = useUnistyles();
  const pulse = useRef(new Animated.Value(0.5)).current;
  useEffect(() => {
    const loop = Animated.loop(
      Animated.sequence([
        Animated.timing(pulse, { toValue: 1, duration: 700, useNativeDriver: false }),
        Animated.timing(pulse, { toValue: 0.5, duration: 700, useNativeDriver: false }),
      ]),
    );
    loop.start();
    return () => loop.stop();
  }, [pulse]);
  return (
    <Animated.View
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
      style={{
        width,
        height,
        ...(height === undefined && { aspectRatio: 1 }),
        borderRadius: round ? 9999 : (radius ?? theme.radius.md),
        backgroundColor: theme.colors.surfaceVariant,
        opacity: pulse,
      }}
    />
  );
}

/** Lines of a list still loading: an artwork's square, if `art`, and two lines of text each. */
export function SkeletonRows({ count, art = 40 }: { count: number; art?: number | null }) {
  const { theme } = useUnistyles();
  return (
    <View style={{ gap: theme.spacing.md, paddingVertical: theme.spacing.xs }}>
      {Array.from({ length: count }, (_, i) => (
        // Placeholders have no identity but their place.
        // biome-ignore lint/suspicious/noArrayIndexKey: as above.
        <View key={i} style={{ flexDirection: 'row', alignItems: 'center', gap: theme.spacing.md }}>
          {art ? <Skeleton width={art} height={art} /> : null}
          <View style={{ flex: 1, gap: theme.spacing.xs }}>
            <Skeleton width="45%" height={12} radius={theme.radius.sm} />
            <Skeleton width="30%" height={10} radius={theme.radius.sm} />
          </View>
        </View>
      ))}
    </View>
  );
}
