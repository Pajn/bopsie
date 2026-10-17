import type { WithImages } from '@bopsie/client/api/schemas/media';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import type { IconName } from '@/ui/Icon';
import { Button, Text } from '@/ui/primitives';
import { usePending } from '@/ui/usePending';
import { Artwork } from './Artwork';

/**
 * The top of an album, artist or playlist page: artwork, names, and play and shuffle buttons,
 * each spinning until what it started has.
 */
export function ItemHeader({
  item,
  title,
  subtitle,
  onPlay,
  onShuffle,
  round,
  placeholder,
}: {
  item: WithImages | null;
  title: string;
  subtitle?: string;
  onPlay: () => Promise<unknown> | undefined;
  onShuffle: () => Promise<unknown> | undefined;
  round?: boolean;
  placeholder?: IconName;
}) {
  const play = usePending(onPlay);
  const shuffle = usePending(onShuffle);
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
      <View style={styles.buttons}>
        <Button
          title="Play"
          loading={play.pending}
          onPress={play.onPress}
          accessibilityState={{ busy: play.pending }}
          testID="item-play"
        />
        <Button
          title="Shuffle"
          variant="secondary"
          loading={shuffle.pending}
          onPress={shuffle.onPress}
          accessibilityState={{ busy: shuffle.pending }}
          testID="item-shuffle"
        />
      </View>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  header: { alignItems: 'center', gap: theme.spacing.md, paddingVertical: theme.spacing.lg },
  names: { gap: theme.spacing.xs },
  center: { textAlign: 'center' },
  buttons: { flexDirection: 'row', gap: theme.spacing.sm },
}));
