import { View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { Artwork } from '@/features/library/Artwork';
import { QueueList } from '@/features/queue/QueueList';
import { Icon } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { type Frame, MorphPanel } from './MorphPanel';
import { describeItem, type HeardQueue } from './nowPlaying';

/** The height of the bar the panel rises from, which the player keeps free for it. */
export const QUEUE_BAR_HEIGHT = 56;

const ARTWORK = 36;

/**
 * The foot of the full player: what plays next, or what the group plays from in place of
 * its queue. It grows into the queue over the whole player; see `MorphPanel`.
 *
 * @param bar where the bar sits in the player, and `player` the player's size.
 */
export function QueuePanel({
  queue,
  bar,
  player,
  ready,
}: {
  queue: HeardQueue;
  bar: Frame;
  player: { width: number; height: number };
  /** Whether the player shows; see `MorphPanel`. */
  ready: boolean;
}) {
  const { theme } = useUnistyles();
  const next = queue.next_item && !queue.source ? describeItem(queue.next_item) : null;
  const label = queue.source ? 'Playing from' : 'Up next';
  const title = queue.source ? queue.source.name : (next?.title ?? 'Nothing else is queued');
  // The album would crowd out the artists on one line; the queue has it.
  const artists = next
    ? queue.next_item?.media_item?.artists.map((artist) => artist.name).join(', ')
    : undefined;

  return (
    <MorphPanel
      from={bar}
      player={player}
      look={{
        radius: theme.radius.md,
        paddingLeft: theme.spacing.sm,
        paddingRight: theme.spacing.md,
        gap: theme.spacing.md,
        chevron: { size: 18, color: theme.colors.textMuted },
      }}
      chevron="chevron-up"
      title="Queue"
      openLabel={`${label}: ${title}. Open the queue`}
      testID="player-queue"
      ready={ready}
      closed={
        <View style={styles.bar}>
          <View style={styles.artwork}>
            {next ? (
              <Artwork item={next.artwork} size={ARTWORK} imageSize={80} />
            ) : (
              <Icon
                name={queue.source ? 'speaker' : 'list'}
                size={18}
                color={theme.colors.textMuted}
              />
            )}
          </View>
          <View style={styles.text}>
            <Text variant="caption" muted style={styles.label}>
              {label}
            </Text>
            <Text numberOfLines={1} style={next ? null : styles.quiet} testID="player-up-next">
              {title}
              {artists ? <Text style={styles.quiet}>{` · ${artists}`}</Text> : null}
            </Text>
          </View>
        </View>
      }
    >
      <QueueList />
    </MorphPanel>
  );
}

const styles = StyleSheet.create((theme) => ({
  bar: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.md },
  artwork: { width: ARTWORK, height: ARTWORK, alignItems: 'center', justifyContent: 'center' },
  text: { flex: 1 },
  label: { textTransform: 'uppercase', letterSpacing: 0.6 },
  quiet: { color: theme.colors.textMuted },
}));
