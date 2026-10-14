import { useUnistyles } from 'react-native-unistyles';
import type { TreeColors } from './JsonTree';

/** The theme's colours for a JSON tree, which as a DOM component cannot read them itself. */
export function useTreeColors(): TreeColors {
  const { theme } = useUnistyles();
  return {
    text: theme.colors.text,
    muted: theme.colors.textMuted,
    string: theme.colors.code.string,
    value: theme.colors.code.value,
  };
}
