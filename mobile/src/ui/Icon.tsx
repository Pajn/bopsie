import Feather from '@expo/vector-icons/Feather';
import { type ColorValue, type StyleProp, View, type ViewStyle } from 'react-native';
import { useUnistyles } from 'react-native-unistyles';
import type { IconName } from './iconNames';

export type { IconName } from './iconNames';

/**
 * Font-backed web fallback. Android and iOS use Icon.native.tsx and bundled
 * native vectors, so they do not wait for this font's async registration.
 */
export function Icon({
  name,
  size = 22,
  color,
  style,
}: {
  name: IconName;
  size?: number;
  color?: ColorValue;
  style?: StyleProp<ViewStyle>;
}) {
  const { theme } = useUnistyles();
  return (
    <View style={[{ width: size, height: size }, style]} pointerEvents="none">
      <Feather
        name={name}
        size={size}
        style={{ lineHeight: size, color: color ?? theme.colors.text }}
      />
    </View>
  );
}
