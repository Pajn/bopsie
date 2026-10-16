import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useDevices, useGroups, usePlayers } from '@/data/admin';
import { arrange, playerRows } from '@/features/players/model';
import { PlayerCard } from '@/features/players/PlayerCard';
import { PlayerLine } from '@/features/players/PlayerLine';
import { describeError } from '@/lib/errors';
import { Card, InlineError, Page, Text } from '@/ui/primitives';
import { SkeletonRows } from '@/ui/Skeleton';

export default function PlayersScreen() {
  const devices = useDevices();
  const players = usePlayers();
  const groups = useGroups();
  const error = devices.error ?? players.error ?? groups.error;
  const rows = playerRows(devices.data ?? [], players.data ?? [], groups.data ?? []);
  const { active, idle, unavailable } = arrange(rows);
  const quiet = [...idle, ...unavailable];
  return (
    <Page title="Players" subtitle="Every speaker the server knows, and what each plays">
      <InlineError message={error ? describeError(error) : null} />
      {devices.isSuccess && rows.length === 0 ? (
        <Text muted>No players yet. Speakers show here once the server finds them.</Text>
      ) : null}
      {devices.isPending ? (
        <Card>
          <SkeletonRows count={4} art={52} />
        </Card>
      ) : null}
      {active.map((group) => (
        <PlayerCard key={group.leader.id} row={group.leader} rows={rows} members={group.members} />
      ))}
      {quiet.length > 0 ? (
        <View style={styles.section}>
          <Text variant="label" muted>
            {active.length > 0 ? `NOT PLAYING · ${quiet.length}` : `${quiet.length} PLAYERS`}
          </Text>
          <Card style={styles.list}>
            {quiet.map((row, i) => (
              <PlayerLine key={row.id} row={row} rows={rows} last={i === quiet.length - 1} />
            ))}
          </Card>
        </View>
      ) : null}
    </Page>
  );
}

const styles = StyleSheet.create((theme) => ({
  section: { gap: theme.spacing.sm },
  list: { padding: 0, gap: 0 },
}));
