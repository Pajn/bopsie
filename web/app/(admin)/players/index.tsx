import { Link } from 'expo-router';
import { Pressable, ScrollView, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useDevices, useGroups, usePlayers } from '@/data/admin';
import { healthTone, type PlayerRow, playerRows } from '@/features/players/model';
import { PlayerControls } from '@/features/players/PlayerControls';
import { describeError } from '@/lib/errors';
import { Badge, Card, InlineError, Stack, Text } from '@/ui/primitives';

function Row({ row, rows }: { row: PlayerRow; rows: PlayerRow[] }) {
  const state = row.player?.playback_state ?? 'idle';
  const group = row.follows
    ? `follows ${row.follows}`
    : row.followers.length > 0
      ? `leads ${row.followers.join(', ')}`
      : null;
  return (
    <Card>
      <View style={styles.header}>
        <Link href={`/players/${encodeURIComponent(row.id)}`} asChild>
          <Pressable>
            <Text variant="heading" style={styles.name}>
              {row.name}
            </Text>
          </Pressable>
        </Link>
        <Stack horizontal gap="xs">
          {row.available ? null : <Badge label="unavailable" tone="danger" />}
          {row.protocols.map((protocol) => (
            <Badge key={protocol} label={protocol} tone="accent" />
          ))}
          <Badge label={row.health} tone={healthTone(row.health)} />
          <Badge label={state} tone={state === 'playing' ? 'good' : 'muted'} />
          {row.strategy ? <Badge label={row.strategy} /> : null}
        </Stack>
      </View>
      {group || row.player?.current_media?.title ? (
        <Text muted>{[group, row.player?.current_media?.title].filter(Boolean).join(' · ')}</Text>
      ) : null}
      {row.available ? <PlayerControls row={row} rows={rows} /> : null}
    </Card>
  );
}

export default function PlayersScreen() {
  const devices = useDevices();
  const players = usePlayers();
  const groups = useGroups();
  const error = devices.error ?? players.error ?? groups.error;
  const rows = playerRows(devices.data ?? [], players.data ?? [], groups.data ?? []);
  return (
    <ScrollView>
      <Stack gap="lg">
        <Text variant="title">Players</Text>
        <InlineError message={error ? describeError(error) : null} />
        {devices.isSuccess && rows.length === 0 ? (
          <Text muted>No players yet. Speakers show here once the server finds them.</Text>
        ) : null}
        {rows.map((row) => (
          <Row key={row.id} row={row} rows={rows} />
        ))}
      </Stack>
    </ScrollView>
  );
}

const styles = StyleSheet.create((theme) => ({
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    flexWrap: 'wrap',
    gap: theme.spacing.sm,
  },
  name: { color: theme.colors.accent },
}));
