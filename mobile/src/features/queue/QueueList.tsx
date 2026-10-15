import type { QueueItem } from '@bopsie/client/api/schemas/player';
import type { ReactNode } from 'react';
import { Alert, Pressable, View } from 'react-native';
import ReorderableList, { useReorderableDrag } from 'react-native-reorderable-list';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { Artwork } from '@/features/library/Artwork';
import { describeItem } from '@/features/player/nowPlaying';
import { useQueue } from '@/features/player/usePlayback';
import { formatDuration } from '@/player/progress';
import { Icon } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { describeQueue } from './queue';
import { useQueueActions, useQueueItems } from './useQueueItems';

type Actions = ReturnType<typeof useQueueActions>;

/**
 * The active group's queue from the current item on, as the full player shows it. What the
 * player already holds stays put; everything after it can be played, dragged into a new
 * order, moved up to play next, or removed.
 */
export function QueueList() {
  const queue = useQueue().data;
  const items = useQueueItems().data;
  const actions = useQueueActions();

  if (!queue || !items) {
    return (
      <View style={styles.list}>
        <Text muted style={styles.notice}>
          {queue === null ? 'Nothing is queued.' : 'Loading the queue…'}
        </Text>
      </View>
    );
  }

  const { held, upNext, firstFree } = describeQueue(queue, items);
  const currentId = queue.current_item?.queue_item_id;

  return (
    <View style={styles.list}>
      <ReorderableList
        data={upNext}
        keyExtractor={(item) => item.queue_item_id}
        renderItem={({ item, index }) => (
          <UpNextRow
            item={item}
            index={firstFree + index}
            firstFree={firstFree}
            actions={actions}
          />
        )}
        onReorder={({ from, to }) => {
          const item = upNext[from];
          if (item) actions.move(item, firstFree + from, firstFree + to);
        }}
        ListHeaderComponent={
          <>
            {queue.source ? (
              <Text muted style={styles.notice} testID="queue-source">
                {`Playing from ${queue.source.name}. Play something here to take the group back.`}
              </Text>
            ) : null}
            {held.map((item) => (
              <QueueRow
                key={item.queue_item_id}
                item={item}
                current={item.queue_item_id === currentId}
                {...(item.queue_item_id !== currentId && { onPress: () => actions.play(item) })}
              />
            ))}
            <Text variant="label" muted style={styles.section}>
              {upNext.length > 0 ? `Up next · ${upNext.length}` : 'Up next'}
            </Text>
          </>
        }
        ListEmptyComponent={
          <Text muted style={styles.notice}>
            Nothing else is queued.
          </Text>
        }
        contentContainerStyle={styles.content}
        testID="queue"
      />
    </View>
  );
}

function UpNextRow({
  item,
  index,
  firstFree,
  actions,
}: {
  item: QueueItem;
  index: number;
  firstFree: number;
  actions: Actions;
}) {
  const { theme } = useUnistyles();
  const drag = useReorderableDrag();
  const title = describeItem(item).title;

  const more = () =>
    Alert.alert(title, undefined, [
      ...(index > firstFree
        ? [{ text: 'Play next', onPress: () => actions.move(item, index, firstFree) }]
        : []),
      { text: 'Remove from queue', style: 'destructive', onPress: () => actions.remove(item) },
      { text: 'Cancel', style: 'cancel' },
    ]);

  return (
    <QueueRow item={item} onPress={() => actions.play(item)} onLongPress={drag}>
      <Pressable
        onPress={more}
        hitSlop={8}
        accessibilityRole="button"
        accessibilityLabel={`More for ${title}`}
        testID={`queue-more-${item.queue_item_id}`}
      >
        <Icon name="more-vertical" size={20} color={theme.colors.textMuted} />
      </Pressable>
      <Pressable
        onPressIn={drag}
        hitSlop={8}
        accessibilityLabel={`Drag to reorder ${title}`}
        testID={`queue-drag-${item.queue_item_id}`}
      >
        <Icon name="menu" size={20} color={theme.colors.textMuted} />
      </Pressable>
    </QueueRow>
  );
}

function QueueRow({
  item,
  current,
  onPress,
  onLongPress,
  children,
}: {
  item: QueueItem;
  current?: boolean;
  onPress?: () => void;
  onLongPress?: () => void;
  children?: ReactNode;
}) {
  const { title, subtitle, artwork, duration } = describeItem(item);
  return (
    <Pressable
      onPress={onPress}
      onLongPress={onLongPress}
      disabled={!onPress}
      style={({ pressed }) => [styles.row, pressed && styles.pressed]}
      accessibilityRole="button"
      accessibilityLabel={current ? `Now playing: ${title}` : `Play ${title}`}
      testID={`queue-item-${item.queue_item_id}`}
    >
      <Artwork item={artwork} size={40} imageSize={80} />
      <View style={styles.text}>
        <Text numberOfLines={1} style={current && styles.current}>
          {title}
        </Text>
        {subtitle ? (
          <Text variant="caption" muted numberOfLines={1}>
            {subtitle}
          </Text>
        ) : null}
      </View>
      {children ??
        (duration ? (
          <Text variant="caption" muted>
            {formatDuration(duration)}
          </Text>
        ) : null)}
    </Pressable>
  );
}

const styles = StyleSheet.create((theme) => ({
  list: { flex: 1, backgroundColor: theme.colors.background },
  content: { paddingVertical: theme.spacing.lg },
  notice: { textAlign: 'center', padding: theme.spacing.lg },
  section: {
    marginTop: theme.spacing.lg,
    marginBottom: theme.spacing.xs,
    paddingHorizontal: theme.spacing.lg,
    letterSpacing: 0.8,
    textTransform: 'uppercase',
  },
  // Opaque, so a row lifted by a drag hides the rows it passes over.
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingHorizontal: theme.spacing.lg,
    paddingVertical: theme.spacing.sm,
    backgroundColor: theme.colors.background,
  },
  pressed: { opacity: 0.6 },
  text: { flex: 1, gap: 2 },
  current: { color: theme.colors.accent, fontWeight: '600' },
}));
