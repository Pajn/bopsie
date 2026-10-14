import { playMedia, type QueueOption } from '@bopsie/client/api/endpoints/players';
import { useState } from 'react';
import { useConnection } from '@/connection/ConnectionProvider';
import { usePlayers } from '@/data/admin';
import { describeError } from '@/lib/errors';
import { Button, Choice, InlineError, Stack, Text } from '@/ui/primitives';
import { useTarget } from './target';

/** Picks the player the library plays to. */
export function TargetPicker() {
  const players = usePlayers();
  const { playerId, setPlayerId } = useTarget();
  const available = (players.data ?? []).filter((p) => p.available && !p.synced_to);
  const current = players.data?.find((p) => p.player_id === playerId);
  return (
    <Stack horizontal gap="sm">
      <Text muted>Plays to</Text>
      <Choice
        title={current?.name ?? 'Choose a player'}
        options={available.map((p) => ({ value: p.player_id, label: p.name }))}
        onChoose={setPlayerId}
      />
    </Stack>
  );
}

/** Play now, or add to the end, on the chosen player's queue. */
export function PlayButtons({ uri, startItem }: { uri: string; startItem?: string }) {
  const { connection } = useConnection();
  const playerId = useTarget((state) => state.playerId);
  const [error, setError] = useState<string | null>(null);
  const play = (option: QueueOption) => {
    if (!playerId) return;
    setError(null);
    playMedia(connection, playerId, uri, { option, ...(startItem && { startItem }) }).catch(
      (e: unknown) => setError(describeError(e)),
    );
  };
  return (
    <Stack horizontal gap="xs">
      <Button title="Play" disabled={!playerId} onPress={() => play('replace')} />
      <Button title="Add" variant="secondary" disabled={!playerId} onPress={() => play('add')} />
      <InlineError message={error} />
    </Stack>
  );
}
