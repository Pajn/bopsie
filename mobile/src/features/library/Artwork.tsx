import { type ImageSize, imageUrl } from '@bopsie/client/api/images';
import type { WithImages } from '@bopsie/client/api/schemas/media';
import { View } from 'react-native';
import { Image } from 'react-native-true-image';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { Icon, type IconName } from '@/ui/Icon';

/** Square artwork through the server's image proxy, with a glyph when an item has none. */
export function Artwork({
  item,
  size,
  imageSize = 256,
  round,
  placeholder = 'disc',
}: {
  item: WithImages;
  size: number;
  imageSize?: ImageSize;
  round?: boolean;
  placeholder?: IconName;
}) {
  const { account } = useConnection();
  const uri = imageUrl(account.serverUrl, item, imageSize);
  const shape = { width: size, height: size, borderRadius: round ? size / 2 : 6 };
  return (
    <View style={[styles.frame, shape]}>
      {uri ? (
        <Image source={{ uri }} style={shape} resizeMode="cover" />
      ) : (
        <Icon name={placeholder} size={size * 0.4} />
      )}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  frame: {
    backgroundColor: theme.colors.surfaceVariant,
    alignItems: 'center',
    justifyContent: 'center',
    overflow: 'hidden',
  },
}));
