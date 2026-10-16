import Feather from '@expo/vector-icons/Feather';
import type { ComponentProps } from 'react';
import { useUnistyles } from 'react-native-unistyles';

export type IconName = ComponentProps<typeof Feather>['name'];

/**
 * A Feather glyph, the set the app draws its icons from, in the theme's text colour by default.
 * It is a character of an icon font, which a screen reader would read out, so it is hidden from
 * one: what it means is said by the label of the button or link that holds it.
 */
export function Icon({
  name,
  size = 18,
  color,
}: {
  name: IconName;
  size?: number;
  color?: string;
}) {
  const { theme } = useUnistyles();
  return (
    <Feather
      name={name}
      size={size}
      color={color ?? theme.colors.text}
      aria-hidden
      accessibilityElementsHidden
      importantForAccessibility="no"
    />
  );
}
