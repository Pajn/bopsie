import {
  joinGroup,
  leaveGroup,
  queueCommand,
  setVolume,
  transferQueue,
} from '@bopsie/client/api/endpoints/players';
import { useState } from 'react';
import { useConnection } from '@/connection/ConnectionProvider';
import { describeError } from '@/lib/errors';
import { Button, Choice, InlineError, Stack, Text } from '@/ui/primitives';
import { type PlayerRow, queueOf } from './model';

/** Everything a player can be told through the API it already has. */
export function PlayerControls({ row, rows }: { row: PlayerRow; rows: PlayerRow[] }) {
  const { connection } = useConnection();
  const [error, setError] = useState<string | null>(null);
  const run = (action: () => Promise<unknown>) => {
    setError(null);
    action().catch((e: unknown) => setError(describeError(e)));
  };
  const player = row.player;
  const queue = queueOf(row);
  const volume = player?.volume_level ?? null;
  const groupable = new Set(player?.can_group_with ?? []);
  const others = rows.filter((other) => other.id !== row.id && other.available);
  const leaders = others.filter((other) => groupable.has(other.id) && other.follows === null);

  return (
    <Stack gap="sm">
      <Stack horizontal gap="xs">
        <Button
          title="⏮"
          variant="secondary"
          onPress={() => run(() => queueCommand(connection, queue, 'previous'))}
        />
        <Button
          title={player?.playback_state === 'playing' ? '⏸' : '▶'}
          onPress={() => run(() => queueCommand(connection, queue, 'play_pause'))}
        />
        <Button
          title="⏭"
          variant="secondary"
          onPress={() => run(() => queueCommand(connection, queue, 'next'))}
        />
        <Button
          title="⏹"
          variant="secondary"
          onPress={() => run(() => queueCommand(connection, queue, 'stop'))}
        />
        <Button
          title="−"
          variant="secondary"
          disabled={volume === null}
          onPress={() => run(() => setVolume(connection, row.id, Math.max(0, (volume ?? 0) - 5)))}
        />
        <Text variant="mono">{volume === null ? 'vol –' : `vol ${volume}`}</Text>
        <Button
          title="+"
          variant="secondary"
          disabled={volume === null}
          onPress={() => run(() => setVolume(connection, row.id, Math.min(100, (volume ?? 0) + 5)))}
        />
        <Button
          title={player?.volume_muted ? 'Unmute' : 'Mute'}
          variant="secondary"
          disabled={!player}
          onPress={() =>
            run(() =>
              connection.command('players/cmd/volume_mute', {
                player_id: row.id,
                muted: !player?.volume_muted,
              }),
            )
          }
        />
      </Stack>
      <Stack horizontal gap="xs">
        {row.follows || row.followers.length > 0 ? (
          <Button
            title={row.follows ? 'Leave group' : 'Ungroup'}
            variant="secondary"
            onPress={() => run(() => leaveGroup(connection, row.id))}
          />
        ) : null}
        <Choice
          title="Join"
          options={leaders.map((other) => ({ value: other.id, label: other.name }))}
          onChoose={(leader) => run(() => joinGroup(connection, row.id, leader))}
        />
        <Choice
          title="Move queue to"
          options={others.map((other) => ({ value: other.id, label: other.name }))}
          onChoose={(target) => run(() => transferQueue(connection, queue, target))}
        />
      </Stack>
      <InlineError message={error} />
    </Stack>
  );
}
