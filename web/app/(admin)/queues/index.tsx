import type { PlayerQueue } from '@bopsie/client/api/schemas/player';
import { Link } from 'expo-router';
import { useState } from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { usePlayers } from '@/data/admin';
import { useQueues } from '@/data/queues';
import { describeItem, itemArtwork } from '@/features/queues/items';
import { describeError } from '@/lib/errors';
import { Artwork } from '@/ui/Artwork';
import { Icon } from '@/ui/Icon';
import { Card, InlineError, Page, Text } from '@/ui/primitives';

/** What a queue holds and does, in a line. */
function summary(queue: PlayerQueue | null | undefined): string {
  if (!queue || queue.items === 0) return 'Empty';
  const items = `${queue.items} ${queue.items === 1 ? 'item' : 'items'}`;
  if (queue.state === 'playing') return `${queue.buffering ? 'Starting' : 'Playing'} · ${items}`;
  return `${queue.state === 'paused' ? 'Paused' : 'Stopped'} · ${items}`;
}

function QueueRow({
  id,
  name,
  queue,
  last,
}: {
  id: string;
  name: string;
  queue: PlayerQueue | null | undefined;
  last: boolean;
}) {
  const { account } = useConnection();
  const { theme } = useUnistyles();
  const [hovered, setHovered] = useState(false);
  const item = queue?.current_item;
  const described = item ? describeItem(item) : null;
  return (
    <Link href={`/queues/${encodeURIComponent(id)}`} asChild>
      <Pressable
        accessibilityRole="link"
        onHoverIn={() => setHovered(true)}
        onHoverOut={() => setHovered(false)}
      >
        {/* Styled inside: the link merges its child's style, which loses unistyles' classes. */}
        <View style={[styles.row, !last && styles.divided, hovered && styles.hovered]}>
          <Artwork
            uri={item ? itemArtwork(account.serverUrl, item, 80) : null}
            size={48}
            placeholder="list"
          />
          <View style={styles.text}>
            <Text style={styles.name} numberOfLines={1}>
              {name}
            </Text>
            <Text numberOfLines={1}>
              {described
                ? [described.title, described.subtitle].filter(Boolean).join(' · ')
                : 'Nothing loaded'}
            </Text>
            <Text variant="caption" muted numberOfLines={1}>
              {summary(queue)}
            </Text>
          </View>
          {queue?.state === 'playing' ? (
            <Icon name="volume-2" size={16} color={theme.colors.accent} />
          ) : null}
          <Icon name="chevron-right" size={16} color={theme.colors.textMuted} />
        </View>
      </Pressable>
    </Link>
  );
}

/** Every queue: each player that is not following another has one. */
export default function QueuesScreen() {
  const players = usePlayers();
  const leaders = (players.data ?? [])
    .filter((p) => !p.synced_to)
    .sort((a, b) => a.name.localeCompare(b.name));
  const queues = useQueues(leaders.map((p) => p.player_id));
  const error = players.error ?? queues.find((q) => q.error)?.error;
  const all = leaders.map((player, i) => ({ player, queue: queues[i]?.data }));
  // Queues that hold something first, playing before the rest; the empty ones in a line.
  const held = all
    .filter(({ queue }) => (queue?.items ?? 0) > 0)
    .sort((a, b) => Number(b.queue?.state === 'playing') - Number(a.queue?.state === 'playing'));
  const empty = all.filter(({ queue }) => queue && queue.items === 0);
  return (
    <Page title="Queues" subtitle="What each player, or group, plays next">
      <InlineError message={error ? describeError(error) : null} />
      {players.isSuccess && leaders.length === 0 ? (
        <Text muted>No queues yet: each player has one once the server finds it.</Text>
      ) : null}
      {held.length > 0 ? (
        <Card style={styles.list}>
          {held.map(({ player, queue }, i) => (
            <QueueRow
              key={player.player_id}
              id={player.player_id}
              name={player.name}
              queue={queue}
              last={i === held.length - 1}
            />
          ))}
        </Card>
      ) : null}
      {empty.length > 0 ? (
        <View style={styles.empty}>
          <Text variant="label" muted>
            EMPTY · {empty.length}
          </Text>
          <View style={styles.names}>
            {empty.map(({ player }, i) => (
              <Link
                key={player.player_id}
                href={`/queues/${encodeURIComponent(player.player_id)}`}
                asChild
              >
                <Pressable accessibilityRole="link">
                  <Text muted>
                    <Text style={styles.emptyName}>{player.name}</Text>
                    {i < empty.length - 1 ? ',' : ''}
                  </Text>
                </Pressable>
              </Link>
            ))}
          </View>
        </View>
      ) : null}
    </Page>
  );
}

const styles = StyleSheet.create((theme) => ({
  list: { padding: 0, gap: 0, overflow: 'hidden' },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingVertical: theme.spacing.md,
    paddingHorizontal: theme.spacing.lg,
  },
  divided: { borderBottomWidth: 1, borderColor: theme.colors.border },
  hovered: { backgroundColor: theme.colors.surfaceVariant },
  text: { flex: 1, minWidth: 0, gap: 1 },
  name: { color: theme.colors.accent, fontWeight: '600' },
  empty: { gap: theme.spacing.xs },
  names: { flexDirection: 'row', flexWrap: 'wrap', columnGap: theme.spacing.sm, rowGap: 2 },
  emptyName: { color: theme.colors.accent },
}));
