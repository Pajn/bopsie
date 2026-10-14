import type { WithImages } from '@bopsie/client/api/schemas/media';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import type { IconName } from '@/ui/Icon';
import { Button, Text } from '@/ui/primitives';
import { Artwork } from './Artwork';

/** The top of an album, artist or playlist page: artwork, names and a play button. */
export function ItemHeader({
  item,
  title,
  subtitle,
  onPlay,
  round,
  placeholder,
}: {
  item: WithImages | null;
  title: string;
  subtitle?: string;
  onPlay: () => void;
  round?: boolean;
  placeholder?: IconName;
}) {
  return (
    <View style={styles.header}>
      <Artwork
        item={item ?? { image: null }}
        size={200}
        imageSize={512}
        {...(round !== undefined && { round })}
        {...(placeholder && { placeholder })}
      />
      <View style={styles.names}>
        <Text variant="heading" style={styles.center} testID="item-title">
          {title}
        </Text>
        {subtitle ? (
          <Text muted style={styles.center}>
            {subtitle}
          </Text>
        ) : null}
      </View>
      <Button title="Play" onPress={onPlay} testID="item-play" />
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  header: { alignItems: 'center', gap: theme.spacing.md, paddingVertical: theme.spacing.lg },
  names: { gap: theme.spacing.xs },
  center: { textAlign: 'center' },
}));
