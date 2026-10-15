import { View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { OutputsScreen } from '@/features/outputs/OutputsScreen';
import { Icon } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { type Frame, MorphPanel } from './MorphPanel';
import { usePlayerColors } from './usePlayerTint';

/** The size of the pill's chevron, which the player's stand-in for the pill makes room for. */
export const PILL_CHEVRON = 14;

/** What the pill says: where the music plays. */
export function PillContent({ name }: { name: string }) {
  const { accent } = usePlayerColors();
  return (
    <View style={styles.pill}>
      <Icon name="speaker" size={14} color={accent} />
      <Text variant="label" style={[styles.name, { color: accent }]} numberOfLines={1}>
        {name}
      </Text>
    </View>
  );
}

/**
 * The full player's pill for where the music plays, which grows into Play on over the whole
 * player; see `MorphPanel`.
 *
 * @param pill where the pill sits in the player, and `player` the player's size.
 */
export function OutputsPanel({
  name,
  pill,
  player,
  ready,
}: {
  name: string;
  pill: Frame;
  player: { width: number; height: number };
  /** Whether the player shows; see `MorphPanel`. */
  ready: boolean;
}) {
  const { theme } = useUnistyles();
  const { accent } = usePlayerColors();
  return (
    <MorphPanel
      from={pill}
      player={player}
      look={{
        radius: pill.height / 2,
        paddingLeft: theme.spacing.md,
        paddingRight: theme.spacing.md,
        gap: theme.spacing.xs,
        chevron: { size: PILL_CHEVRON, color: accent },
      }}
      chevron="chevron-down"
      title="Play on"
      openLabel={`Playing on ${name}. Choose where to play`}
      testID="player-outputs"
      ready={ready}
      closed={<PillContent name={name} />}
    >
      <OutputsScreen />
    </MorphPanel>
  );
}

const styles = StyleSheet.create((theme) => ({
  pill: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.xs },
  name: { flexShrink: 1 },
}));
