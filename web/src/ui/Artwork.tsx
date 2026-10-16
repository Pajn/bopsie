import { Image, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { Icon, type IconName } from './Icon';

/** Square artwork from `uri`, rounded or round, with a glyph in its place when there is none. */
export function Artwork({
  uri,
  size,
  round,
  placeholder = 'disc',
}: {
  uri: string | null | undefined;
  size: number;
  round?: boolean;
  placeholder?: IconName;
}) {
  const { theme } = useUnistyles();
  const shape = {
    width: size,
    height: size,
    borderRadius: round ? size / 2 : Math.min(8, size / 6),
  };
  return (
    <View style={[styles.frame, shape]}>
      {uri ? (
        <Image source={{ uri }} style={shape} resizeMode="cover" />
      ) : (
        <Icon name={placeholder} size={Math.min(size * 0.4, 48)} color={theme.colors.textMuted} />
      )}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  frame: {
    alignItems: 'center',
    justifyContent: 'center',
    overflow: 'hidden',
    backgroundColor: theme.colors.surfaceVariant,
  },
}));
