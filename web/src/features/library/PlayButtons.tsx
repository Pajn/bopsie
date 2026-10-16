import { playMedia, type QueueOption } from '@bopsie/client/api/endpoints/players';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { IconButton } from '@/ui/controls';
import { Button, Choice, Text } from '@/ui/primitives';
import { useRun } from '@/ui/Toast';
import { usePlayTarget } from './target';

/** Picks the player the library plays to. */
export function TargetPicker() {
  const { player, leaders, setPlayerId } = usePlayTarget();
  return (
    <View style={styles.target}>
      <Text muted>Plays on</Text>
      <Choice
        title={player?.name ?? 'No player'}
        label={player ? `Plays on ${player.name}` : 'Plays on'}
        options={leaders.map((p) => ({ value: p.player_id, label: p.name }))}
        onChoose={setPlayerId}
      />
    </View>
  );
}

/** Plays `uri`, or adds it to the end, on the chosen player's queue, saying which it did. */
export function usePlay(uri: string, startItem?: string) {
  const { connection } = useConnection();
  const { player } = usePlayTarget();
  const run = useRun();
  const play = (option: QueueOption) => {
    if (!player) return;
    run(
      () =>
        playMedia(connection, player.player_id, uri, { option, ...(startItem && { startItem }) }),
      option === 'add' ? `Added to ${player.name}'s queue` : `Playing on ${player.name}`,
    );
  };
  return { play, target: player?.name ?? null };
}

/** Play now, or add to the end, as icons: for a row of a list. */
export function PlayButtons({ uri, startItem }: { uri: string; startItem?: string }) {
  const { play, target } = usePlay(uri, startItem);
  return (
    <View style={styles.row}>
      <IconButton
        icon="play"
        label={target ? `Play on ${target}` : 'Play'}
        variant="ghost"
        size="sm"
        disabled={!target}
        onPress={() => play('replace')}
      />
      <IconButton
        icon="plus"
        label={target ? `Add to ${target}'s queue` : 'Add to queue'}
        variant="ghost"
        size="sm"
        disabled={!target}
        onPress={() => play('add')}
      />
    </View>
  );
}

/** Play now, or add to the end, as buttons with words: for the head of an item's page. */
export function PlayActions({ uri }: { uri: string }) {
  const { play, target } = usePlay(uri);
  return (
    <View style={styles.actions}>
      <Button title="Play" icon="play" disabled={!target} onPress={() => play('replace')} />
      <Button
        title="Add to queue"
        icon="plus"
        variant="secondary"
        disabled={!target}
        onPress={() => play('add')}
      />
      <TargetPicker />
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  target: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.sm },
  row: { flexDirection: 'row', alignItems: 'center', gap: 2 },
  actions: {
    flexDirection: 'row',
    alignItems: 'center',
    flexWrap: 'wrap',
    gap: theme.spacing.sm,
  },
}));
