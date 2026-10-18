import { requireNativeView } from 'expo';
import { type ColorValue, processColor, type StyleProp, type ViewStyle } from 'react-native';
import { useUnistyles } from 'react-native-unistyles';
import type { IconName } from './iconNames';

export type { IconName } from './iconNames';

const NativeIcon = requireNativeView<{
  name: IconName;
  tint: ReturnType<typeof processColor>;
  style?: StyleProp<ViewStyle>;
  pointerEvents?: 'none';
}>('BopsieIcons');

/** Bundled native vector, assigned synchronously during the native prop update. */
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
    <NativeIcon
      name={name}
      tint={processColor(color ?? theme.colors.text)}
      style={[{ width: size, height: size }, style]}
      pointerEvents="none"
    />
  );
}
