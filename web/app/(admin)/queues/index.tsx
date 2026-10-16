import { Link } from 'expo-router';
import { Pressable, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { usePlayers } from '@/data/admin';
import { useQueues } from '@/data/queues';
import { describeError } from '@/lib/errors';
import { Badge, Card, InlineError, Page, Text } from '@/ui/primitives';

/** Every queue: each player that is not following another has one. */
export default function QueuesScreen() {
  const players = usePlayers();
  const leaders = (players.data ?? [])
    .filter((p) => !p.synced_to)
    .sort((a, b) => a.name.localeCompare(b.name));
  const queues = useQueues(leaders.map((p) => p.player_id));
  const error = players.error ?? queues.find((q) => q.error)?.error;
  return (
    <Page title={'Queues'}>
      <InlineError message={error ? describeError(error) : null} />
      <Card>
        {leaders.map((player, i) => {
          const queue = queues[i]?.data;
          return (
            <Link
              key={player.player_id}
              href={`/queues/${encodeURIComponent(player.player_id)}`}
              asChild
            >
              <Pressable style={styles.row}>
                <View style={styles.name}>
                  <Text style={styles.link}>{player.name}</Text>
                  <Text variant="caption" muted numberOfLines={1}>
                    {queue?.current_item?.name ?? 'Nothing loaded'}
                  </Text>
                </View>
                <Text muted>{queue ? `${queue.items} items` : ''}</Text>
                <Badge
                  label={queue?.state ?? 'idle'}
                  tone={queue?.state === 'playing' ? 'good' : 'muted'}
                />
              </Pressable>
            </Link>
          );
        })}
      </Card>
    </Page>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingVertical: theme.spacing.sm,
    borderBottomWidth: 1,
    borderColor: theme.colors.border,
  },
  name: { flex: 1, minWidth: 0 },
  link: { color: theme.colors.accent },
}));
