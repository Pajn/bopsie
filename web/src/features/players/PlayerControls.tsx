import {
  joinGroup,
  leaveGroup,
  queueCommand,
  setVolume,
  transferQueue,
} from '@bopsie/client/api/endpoints/players';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { IconButton, VolumeSlider } from '@/ui/controls';
import { Button, Choice, Stack } from '@/ui/primitives';
import { useRun } from '@/ui/Toast';
import { type PlayerRow, queueOf } from './model';

/** Everything a player can be told through the API it already has. */
export function PlayerControls({ row, rows }: { row: PlayerRow; rows: PlayerRow[] }) {
  const { connection } = useConnection();
  const run = useRun();
  const player = row.player;
  const queue = queueOf(row);
  const volume = player?.volume_level ?? null;
  const groupable = new Set(player?.can_group_with ?? []);
  const others = rows.filter((other) => other.id !== row.id && other.available);
  const leaders = others.filter((other) => groupable.has(other.id) && other.follows === null);

  const playing = player?.playback_state === 'playing';
  const command = (name: 'previous' | 'play_pause' | 'next' | 'stop') =>
    run(() => queueCommand(connection, queue, name));

  return (
    <Stack gap="sm">
      <View style={styles.transport}>
        <Stack horizontal gap="xs" style={styles.buttons}>
          <IconButton
            icon="skip-back"
            label="Previous"
            variant="ghost"
            onPress={() => command('previous')}
          />
          <IconButton
            icon={playing ? 'pause' : 'play'}
            label={playing ? 'Pause' : 'Play'}
            variant="primary"
            onPress={() => command('play_pause')}
          />
          <IconButton
            icon="skip-forward"
            label="Next"
            variant="ghost"
            onPress={() => command('next')}
          />
          <IconButton icon="square" label="Stop" variant="ghost" onPress={() => command('stop')} />
        </Stack>
        <View style={styles.volume}>
          <VolumeSlider
            value={volume}
            muted={player?.volume_muted ?? null}
            label={row.name}
            onChange={(level) => run(() => setVolume(connection, row.id, level))}
            onMute={(muted) =>
              run(() => connection.command('players/cmd/volume_mute', { player_id: row.id, muted }))
            }
          />
        </View>
      </View>
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
    </Stack>
  );
}

const styles = StyleSheet.create((theme) => ({
  // The buttons and the volume side by side, the volume below them on a phone.
  transport: {
    flexDirection: { xs: 'column', md: 'row' },
    alignItems: { xs: 'stretch', md: 'center' },
    gap: theme.spacing.md,
  },
  buttons: { alignItems: 'center' },
  volume: { flex: 1, maxWidth: { xs: undefined, md: 320 } },
}));
