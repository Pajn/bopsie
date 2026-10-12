import Feather from '@expo/vector-icons/Feather';
import type { ComponentProps } from 'react';
import { type ColorValue, type StyleProp, View, type ViewStyle } from 'react-native';
import { useUnistyles } from 'react-native-unistyles';

export type IconName = ComponentProps<typeof Feather>['name'];

/** A Feather glyph sized to a square box, tinted with the theme's text color by default. */
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
        color={color ?? theme.colors.text}
        style={{ lineHeight: size }}
      />
    </View>
  );
}
