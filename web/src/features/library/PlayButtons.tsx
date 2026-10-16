import { playMedia, type QueueOption } from '@bopsie/client/api/endpoints/players';
import { useState } from 'react';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { describeError } from '@/lib/errors';
import { IconButton } from '@/ui/controls';
import { Button, Choice, InlineError, Text } from '@/ui/primitives';
import { usePlayTarget } from './target';

/** Picks the player the library plays to. */
export function TargetPicker() {
  const { player, leaders, setPlayerId } = usePlayTarget();
  return (
    <View style={styles.target}>
      <Text muted>Plays on</Text>
      <Choice
        title={player?.name ?? 'No player'}
        options={leaders.map((p) => ({ value: p.player_id, label: p.name }))}
        onChoose={setPlayerId}
      />
    </View>
  );
}

/** Plays `uri`, or adds it to the end, on the chosen player's queue. */
function usePlay(uri: string, startItem?: string) {
  const { connection } = useConnection();
  const { player } = usePlayTarget();
  const [error, setError] = useState<string | null>(null);
  const play = (option: QueueOption) => {
    if (!player) return;
    setError(null);
    playMedia(connection, player.player_id, uri, {
      option,
      ...(startItem && { startItem }),
    }).catch((e: unknown) => setError(describeError(e)));
  };
  return { play, error, target: player?.name ?? null };
}

/** Play now, or add to the end, as icons: for a row of a list. */
export function PlayButtons({ uri, startItem }: { uri: string; startItem?: string }) {
  const { play, error, target } = usePlay(uri, startItem);
  return (
    <View style={styles.row}>
      <InlineError message={error} />
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
  const { play, error, target } = usePlay(uri);
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
      <InlineError message={error} />
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
