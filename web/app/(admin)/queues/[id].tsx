import {
  moveQueueItem,
  playQueueItem,
  queueCommand,
  removeQueueItem,
  seek,
} from '@bopsie/client/api/endpoints/players';
import { useLocalSearchParams } from 'expo-router';
import { useEffect, useState } from 'react';
import { ScrollView, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { usePlayers } from '@/data/admin';
import { useQueue, useQueueItems } from '@/data/queues';
import JsonTree from '@/debug/JsonTree';
import { useTreeColors } from '@/debug/treeColors';
import { describeError } from '@/lib/errors';
import { duration, elapsedNow } from '@/lib/time';
import { Badge, Button, Card, Facts, InlineError, Stack, Text } from '@/ui/primitives';

const REPEAT = ['off', 'all', 'one'] as const;

/** The clock, a second at a time, for positions that move while playing. */
function useNow(): number {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  return now;
}

export default function QueueScreen() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { connection } = useConnection();
  const colors = useTreeColors();
  const players = usePlayers();
  const queue = useQueue(id);
  const items = useQueueItems(id);
  const now = useNow();
  const [error, setError] = useState<string | null>(null);
  const run = (action: () => Promise<unknown>) => {
    setError(null);
    action().catch((e: unknown) => setError(describeError(e)));
  };
  const name = players.data?.find((p) => p.player_id === id)?.name ?? id;
  const q = queue.data;
  const elapsed = q ? elapsedNow(q, now) : 0;
  const length = q?.current_item?.duration ?? 0;
  // The server refuses moves and removals up to what the player already holds.
  const held = Math.max(q?.current_index ?? -1, q?.index_in_buffer ?? -1);
  const loadError = queue.error ?? items.error;

  return (
    <ScrollView>
      <Stack gap="lg">
        <Text variant="title">{name}</Text>
        <InlineError message={loadError ? describeError(loadError) : null} />
        {q ? (
          <Card title={q.current_item?.name ?? 'Nothing loaded'}>
            <Facts
              rows={[
                [
                  'State',
                  <Badge
                    key="state"
                    label={q.state}
                    tone={q.state === 'playing' ? 'good' : 'muted'}
                  />,
                ],
                ['Position', `${duration(elapsed)} / ${duration(length)}`],
                ['Items', String(q.items)],
                ['Shuffle', q.shuffle_enabled ? 'on' : 'off'],
                ['Repeat', q.repeat_mode],
              ]}
            />
            <Stack horizontal gap="xs">
              <Button
                title="⏮"
                variant="secondary"
                onPress={() => run(() => queueCommand(connection, id, 'previous'))}
              />
              <Button
                title="−15 s"
                variant="secondary"
                onPress={() => run(() => seek(connection, id, elapsed - 15))}
              />
              <Button
                title={q.state === 'playing' ? '⏸' : '▶'}
                onPress={() => run(() => queueCommand(connection, id, 'play_pause'))}
              />
              <Button
                title="+15 s"
                variant="secondary"
                onPress={() => run(() => seek(connection, id, elapsed + 15))}
              />
              <Button
                title="⏭"
                variant="secondary"
                onPress={() => run(() => queueCommand(connection, id, 'next'))}
              />
              <Button
                title="⏹"
                variant="secondary"
                onPress={() => run(() => queueCommand(connection, id, 'stop'))}
              />
              <Button
                title={q.shuffle_enabled ? 'Shuffle off' : 'Shuffle on'}
                variant="secondary"
                onPress={() =>
                  run(() =>
                    connection.command('player_queues/shuffle', {
                      queue_id: id,
                      shuffle_enabled: !q.shuffle_enabled,
                    }),
                  )
                }
              />
              <Button
                title={`Repeat ${q.repeat_mode}`}
                variant="secondary"
                onPress={() => {
                  const next =
                    REPEAT[(REPEAT.indexOf(q.repeat_mode as (typeof REPEAT)[number]) + 1) % 3];
                  run(() =>
                    connection.command('player_queues/repeat', { queue_id: id, repeat_mode: next }),
                  );
                }}
              />
              <Button
                title="Clear"
                variant="secondary"
                onPress={() => run(() => queueCommand(connection, id, 'clear'))}
              />
            </Stack>
            <InlineError message={error} />
          </Card>
        ) : null}
        <Card title="Items">
          {items.data?.length === 0 ? <Text muted>The queue is empty.</Text> : null}
          {items.data?.map((item, index) => (
            <View key={item.queue_item_id} style={styles.row(index === q?.current_index)}>
              <Text variant="mono" muted style={styles.index}>
                {index + 1}
              </Text>
              <View style={styles.name}>
                <Text numberOfLines={1}>{item.name}</Text>
              </View>
              <Text variant="mono" muted>
                {duration(item.duration ?? 0)}
              </Text>
              <Button
                title="Play"
                variant="secondary"
                onPress={() => run(() => playQueueItem(connection, id, item.queue_item_id))}
              />
              <Button
                title="↑"
                variant="secondary"
                disabled={index <= held + 1}
                onPress={() => run(() => moveQueueItem(connection, id, item.queue_item_id, -1))}
              />
              <Button
                title="↓"
                variant="secondary"
                disabled={index <= held}
                onPress={() => run(() => moveQueueItem(connection, id, item.queue_item_id, 1))}
              />
              <Button
                title="✕"
                variant="secondary"
                disabled={index <= held}
                onPress={() => run(() => removeQueueItem(connection, id, item.queue_item_id))}
              />
            </View>
          ))}
        </Card>
        {q ? (
          <Card title="Raw">
            <JsonTree value={JSON.parse(JSON.stringify(q))} colors={colors} />
          </Card>
        ) : null}
      </Stack>
    </ScrollView>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: (current: boolean) => ({
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.sm,
    paddingVertical: theme.spacing.xs,
    paddingHorizontal: theme.spacing.xs,
    borderRadius: theme.radius.sm,
    backgroundColor: current ? theme.colors.surfaceVariant : 'transparent',
  }),
  index: { width: 32, textAlign: 'right' },
  name: { flex: 1, minWidth: 0 },
}));
