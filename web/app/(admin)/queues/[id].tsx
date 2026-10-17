import {
  moveQueueItem,
  playQueueItem,
  queueCommand,
  removeQueueItem,
  seek,
} from '@bopsie/client/api/endpoints/players';
import type { PlayerQueue, QueueItem } from '@bopsie/client/api/schemas/player';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQueryClient } from '@tanstack/react-query';
import { useLocalSearchParams } from 'expo-router';
import { useState } from 'react';
import { View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { usePlayers } from '@/data/admin';
import { useQueue, useQueueItems } from '@/data/queues';
import JsonTree from '@/debug/JsonTree';
import { useTreeColors } from '@/debug/treeColors';
import { BookControls, playsABook, QueueChapters } from '@/features/audiobooks/BookControls';
import { Credits } from '@/features/library/Credits';
import { describeItem, itemArtwork } from '@/features/queues/items';
import { makeRoom, moveItem } from '@/features/queues/reorder';
import { type Drag, useDragReorder } from '@/features/queues/useDragReorder';
import { describeError } from '@/lib/errors';
import { duration, elapsedNow } from '@/lib/time';
import { useNow } from '@/lib/useNow';
import { Artwork } from '@/ui/Artwork';
import { Disclosure, IconButton, SeekBar } from '@/ui/controls';
import { Icon } from '@/ui/Icon';
import { Button, Card, InlineError, Page, Text } from '@/ui/primitives';
import { SkeletonRows } from '@/ui/Skeleton';
import { useRun } from '@/ui/Toast';

const REPEAT = ['off', 'all', 'one'] as const;

type Run = (action: () => Promise<unknown>) => void;

function state(queue: PlayerQueue): string {
  if (queue.state === 'playing') return queue.buffering ? 'Starting' : 'Playing';
  return queue.state === 'paused' ? 'Paused' : 'Stopped';
}

/** What plays, where in it, and the controls for the queue as a whole. */
function NowPlaying({ id, queue, run }: { id: string; queue: PlayerQueue; run: Run }) {
  const { account, connection } = useConnection();
  const now = useNow();
  const item = queue.current_item;
  const elapsed = elapsedNow(queue, now);
  const length = item?.duration ?? 0;
  const described = item ? describeItem(item) : null;
  const repeat = queue.repeat_mode as (typeof REPEAT)[number];
  return (
    <Card>
      <View style={styles.now}>
        <Artwork uri={item ? itemArtwork(account.serverUrl, item, 256) : null} size={96} />
        <View style={styles.nowText}>
          <Text variant="caption" muted>
            {state(queue)}
          </Text>
          <Text variant="heading" numberOfLines={2}>
            {described?.title ?? 'Nothing loaded'}
          </Text>
          {item?.media_item ? (
            <Credits track={item.media_item} />
          ) : described?.subtitle ? (
            <Text muted numberOfLines={1}>
              {described.subtitle}
            </Text>
          ) : null}
        </View>
      </View>
      {item ? (
        <SeekBar
          elapsed={elapsed}
          length={length}
          onSeek={(seconds) => run(() => seek(connection, id, seconds))}
        />
      ) : null}
      <View style={styles.transport}>
        <IconButton
          icon="shuffle"
          label={queue.shuffle_enabled ? 'Shuffle off' : 'Shuffle on'}
          variant="ghost"
          active={queue.shuffle_enabled}
          onPress={() =>
            run(() =>
              connection.command('player_queues/shuffle', {
                queue_id: id,
                shuffle_enabled: !queue.shuffle_enabled,
              }),
            )
          }
        />
        <IconButton
          icon="skip-back"
          label="Previous"
          variant="ghost"
          onPress={() => run(() => queueCommand(connection, id, 'previous'))}
        />
        <IconButton
          icon="rotate-ccw"
          label="Back 15 seconds"
          variant="ghost"
          disabled={!item}
          onPress={() => run(() => seek(connection, id, Math.max(0, elapsed - 15)))}
        />
        <IconButton
          icon={queue.state === 'playing' ? 'pause' : 'play'}
          label={queue.state === 'playing' ? 'Pause' : 'Play'}
          variant="primary"
          size="lg"
          onPress={() => run(() => queueCommand(connection, id, 'play_pause'))}
        />
        <IconButton
          icon="rotate-cw"
          label="Forward 15 seconds"
          variant="ghost"
          disabled={!item}
          onPress={() => run(() => seek(connection, id, elapsed + 15))}
        />
        <IconButton
          icon="skip-forward"
          label="Next"
          variant="ghost"
          onPress={() => run(() => queueCommand(connection, id, 'next'))}
        />
        <IconButton
          icon="repeat"
          label={`Repeat ${repeat}`}
          variant="ghost"
          active={repeat !== 'off'}
          {...(repeat === 'one' && { mark: '1' })}
          onPress={() => {
            const next = REPEAT[(REPEAT.indexOf(repeat) + 1) % REPEAT.length];
            run(() =>
              connection.command('player_queues/repeat', { queue_id: id, repeat_mode: next }),
            );
          }}
        />
      </View>
      <BookControls queueId={id} queue={queue} run={run} />
    </Card>
  );
}

/**
 * An item of the queue, with what can be done to it beside it, or below it on a phone. One
 * the player does not hold yet has a handle to drag it into a new order by.
 */
function ItemRow({
  id,
  item,
  index,
  current,
  held,
  run,
  drag,
  rowRef,
  onDragStart,
}: {
  id: string;
  item: QueueItem;
  index: number;
  current: number;
  held: number;
  run: Run;
  drag: Drag | null;
  rowRef: (node: object | null) => void;
  onDragStart: (clientY: number) => void;
}) {
  const { account, connection } = useConnection();
  const { theme } = useUnistyles();
  const [open, setOpen] = useState(false);
  const { title, subtitle } = describeItem(item);
  const isCurrent = index === current;
  const lifted = drag?.from === index;
  const shift = drag
    ? lifted
      ? drag.offset
      : makeRoom(index, drag.from, drag.to, drag.height)
    : 0;
  const actions = (
    <>
      <IconButton
        icon="play"
        label={`Play ${title} now`}
        variant="ghost"
        size="sm"
        onPress={() => run(() => playQueueItem(connection, id, item.queue_item_id))}
      />
      <IconButton
        icon="arrow-up"
        label={`Move ${title} up`}
        variant="ghost"
        size="sm"
        disabled={index <= held + 1}
        onPress={() => run(() => moveQueueItem(connection, id, item.queue_item_id, -1))}
      />
      <IconButton
        icon="arrow-down"
        label={`Move ${title} down`}
        variant="ghost"
        size="sm"
        disabled={index <= held}
        onPress={() => run(() => moveQueueItem(connection, id, item.queue_item_id, 1))}
      />
      <IconButton
        icon="x"
        label={`Remove ${title}`}
        variant="ghost"
        size="sm"
        disabled={index <= held}
        onPress={() => run(() => removeQueueItem(connection, id, item.queue_item_id))}
      />
    </>
  );
  return (
    <View
      ref={rowRef}
      style={[
        styles.item(isCurrent, index < current),
        drag && (lifted ? styles.lifted : styles.makingRoom),
        shift !== 0 && { transform: [{ translateY: shift }] },
      ]}
    >
      <View style={styles.itemRow}>
        <View style={styles.index}>
          {isCurrent ? (
            <Icon name="volume-2" size={14} color={theme.colors.accent} />
          ) : (
            <Text variant="caption" muted>
              {index + 1}
            </Text>
          )}
        </View>
        <Artwork uri={itemArtwork(account.serverUrl, item, 80)} size={40} />
        <View style={styles.itemText}>
          <Text numberOfLines={1} style={isCurrent && styles.currentTitle}>
            {title}
          </Text>
          {item.media_item ? (
            <Credits track={item.media_item} variant="caption" />
          ) : subtitle ? (
            <Text variant="caption" muted numberOfLines={1}>
              {subtitle}
            </Text>
          ) : null}
        </View>
        <Text variant="caption" muted style={styles.duration}>
          {item.duration ? duration(item.duration) : ''}
        </Text>
        <View style={styles.wideActions}>{actions}</View>
        <View style={styles.narrowOnly}>
          <IconButton
            icon="more-horizontal"
            label={open ? `Fewer actions for ${title}` : `More actions for ${title}`}
            variant="ghost"
            size="sm"
            active={open}
            onPress={() => setOpen(!open)}
          />
        </View>
        {index > held ? (
          <View
            accessibilityLabel={`Drag to reorder ${title}`}
            {...({ title: `Drag to reorder ${title}` } as object)}
            onPointerDown={(event) => {
              if (event.nativeEvent.button !== 0) return;
              // Neither selects text nor, on a phone, scrolls the page.
              event.preventDefault();
              onDragStart(event.nativeEvent.clientY);
            }}
            style={[styles.handle, lifted && styles.grabbing]}
            testID={`queue-drag-${item.queue_item_id}`}
          >
            <Icon name="menu" size={16} color={theme.colors.textMuted} />
          </View>
        ) : (
          <View style={styles.handleSpace} />
        )}
      </View>
      {open ? <View style={styles.narrowActions}>{actions}</View> : null}
    </View>
  );
}

export default function QueueScreen() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const { account, connection } = useConnection();
  const queryClient = useQueryClient();
  const colors = useTreeColors();
  const players = usePlayers();
  const queue = useQueue(id);
  const items = useQueueItems(id);
  const run: Run = useRun();
  const name = players.data?.find((p) => p.player_id === id)?.name ?? id;
  const q = queue.data;
  // The server refuses moves and removals up to what the player already holds.
  const held = Math.max(q?.current_index ?? -1, q?.index_in_buffer ?? -1);
  const loadError = queue.error ?? items.error;
  const count = items.data?.length ?? 0;

  // A dropped item shows where it was dropped at once; the server's own order replaces it when
  // the move settles, so one it refuses is withdrawn.
  const reorder = useDragReorder(held + 1, count - 1, (from, to) => {
    const item = items.data?.[from];
    if (!item) return;
    const key = queryKeys.queueItems(account.id, id);
    // A fetch already under way would bring back the order from before the move.
    void queryClient.cancelQueries({ queryKey: key });
    queryClient.setQueryData<QueueItem[]>(key, (all) => all && moveItem(all, from, to));
    run(() =>
      moveQueueItem(connection, id, item.queue_item_id, to - from).finally(
        () => void queryClient.invalidateQueries({ queryKey: key }),
      ),
    );
  });

  return (
    <Page
      title={name}
      back={{ href: '/queues', label: 'Queues' }}
      actions={
        count > 0 ? (
          <Button
            title="Clear queue"
            variant="secondary"
            onPress={() => run(() => queueCommand(connection, id, 'clear'))}
          />
        ) : undefined
      }
    >
      <InlineError message={loadError ? describeError(loadError) : null} />
      {q ? <NowPlaying id={id} queue={q} run={run} /> : null}
      {q && playsABook(q) && (q.current_item?.media_item?.metadata.chapters.length ?? 0) > 0 ? (
        <Card title="Chapters">
          <QueueChapters queueId={id} queue={q} run={run} />
        </Card>
      ) : null}
      {queue.isPending ? (
        <Card>
          <SkeletonRows count={1} art={96} />
        </Card>
      ) : null}
      {items.isPending ? (
        <Card>
          <SkeletonRows count={6} art={40} />
        </Card>
      ) : null}
      {items.isPending ? null : (
        <Card title={count > 0 ? `${count} ${count === 1 ? 'item' : 'items'}` : 'Items'}>
          {items.data?.length === 0 ? (
            <Text muted>The queue is empty. Play something from the library to fill it.</Text>
          ) : null}
          <View ref={reorder.listRef}>
            {items.data?.map((item, index) => (
              <ItemRow
                key={item.queue_item_id}
                id={id}
                item={item}
                index={index}
                current={q?.current_index ?? -1}
                held={held}
                run={run}
                drag={reorder.drag}
                rowRef={reorder.rowRef(index)}
                onDragStart={(clientY) => reorder.start(index, clientY)}
              />
            ))}
          </View>
        </Card>
      )}
      {q ? (
        <Disclosure title="Raw" hint="What the server reports">
          <JsonTree value={JSON.parse(JSON.stringify(q))} colors={colors} />
        </Disclosure>
      ) : null}
    </Page>
  );
}

const styles = StyleSheet.create((theme) => ({
  now: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.lg },
  nowText: { flex: 1, minWidth: 0, gap: 2 },
  transport: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: { xs: 'space-between', md: 'center' },
    gap: { xs: 0, md: theme.spacing.md },
  },
  item: (current: boolean, played: boolean) => ({
    paddingVertical: theme.spacing.xs,
    paddingHorizontal: theme.spacing.xs,
    marginHorizontal: -theme.spacing.xs,
    borderRadius: theme.radius.md,
    backgroundColor: current ? theme.colors.surfaceVariant : 'transparent',
    opacity: played ? 0.55 : 1,
  }),
  // Over the rows it passes, which slide aside for it as it goes.
  lifted: {
    position: 'relative',
    zIndex: 1,
    backgroundColor: theme.colors.surface,
    boxShadow: '0 4px 16px rgba(0, 0, 0, 0.2)',
  },
  makingRoom: { transitionProperty: 'transform', transitionDuration: '150ms' } as object,
  handle: {
    width: 28,
    height: 28,
    alignItems: 'center',
    justifyContent: 'center',
    cursor: 'grab',
    touchAction: 'none',
    userSelect: 'none',
  } as object,
  grabbing: { cursor: 'grabbing' } as object,
  handleSpace: { width: 28 },
  itemRow: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.sm },
  index: { width: 24, alignItems: 'center' },
  itemText: { flex: 1, minWidth: 0 },
  currentTitle: { color: theme.colors.accent, fontWeight: '600' },
  duration: {
    display: { xs: 'none', md: 'flex' },
    width: 44,
    textAlign: 'right',
    fontVariant: ['tabular-nums'],
  },
  wideActions: { display: { xs: 'none', md: 'flex' }, flexDirection: 'row', gap: 2 },
  narrowOnly: { display: { xs: 'flex', md: 'none' } },
  narrowActions: {
    display: { xs: 'flex', md: 'none' },
    flexDirection: 'row',
    justifyContent: 'flex-end',
    gap: theme.spacing.xs,
    paddingTop: theme.spacing.xs,
  },
}));
