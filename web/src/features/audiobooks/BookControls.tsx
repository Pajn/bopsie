import { seek, setPlaybackSpeed, setSleepTimer } from '@bopsie/client/api/endpoints/players';
import type { Chapter } from '@bopsie/client/api/schemas/media';
import type { PlayerQueue } from '@bopsie/client/api/schemas/player';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { duration } from '@/lib/time';
import { useNow } from '@/lib/useNow';
import { Icon } from '@/ui/Icon';
import { Choice, Text } from '@/ui/primitives';
import { chapterLength, SLEEP_MINUTES, SPEEDS, sleepLabel, speedLabel } from './model';

type Run = (action: () => Promise<unknown>) => void;

/** Whether what a queue plays now is an audiobook. */
export const playsABook = (queue: PlayerQueue | null | undefined) =>
  queue?.current_item?.media_item?.media_type === 'audiobook';

/** How fast the book plays, and the sleep timer: the speed for a book only. */
export function BookControls({
  queueId,
  queue,
  run,
}: {
  queueId: string;
  queue: PlayerQueue;
  run: Run;
}) {
  const { connection } = useConnection();
  const now = useNow();
  const book = playsABook(queue);
  const chapters = queue.current_item?.media_item?.metadata.chapters ?? [];
  return (
    <View style={styles.row}>
      {book ? (
        <Choice
          title={speedLabel(queue.playback_speed)}
          label="Playback speed"
          options={SPEEDS.map((speed) => ({ value: String(speed), label: speedLabel(speed) }))}
          onChoose={(value) => run(() => setPlaybackSpeed(connection, queueId, Number(value)))}
        />
      ) : null}
      <Choice
        title={sleepLabel(queue, now)}
        label="Sleep timer"
        options={[
          ...SLEEP_MINUTES.map((minutes) => ({
            value: String(minutes),
            label: `In ${minutes} min`,
          })),
          ...(book || chapters.length > 0
            ? [{ value: 'end_of_chapter', label: 'At the end of the chapter' }]
            : [{ value: 'end_of_chapter', label: 'At the end of this item' }]),
          ...(queue.sleep_at != null || queue.sleep_end_of_chapter
            ? [{ value: 'off', label: 'Off' }]
            : []),
        ]}
        onChoose={(value) =>
          run(() =>
            setSleepTimer(
              connection,
              queueId,
              value === 'off'
                ? null
                : value === 'end_of_chapter'
                  ? 'end_of_chapter'
                  : { minutes: Number(value) },
            ),
          )
        }
      />
    </View>
  );
}

/**
 * A book's chapters, with how long each runs. `current` marks the one playing; with `onSeek`,
 * a chapter plays from its start when chosen.
 */
export function ChapterList({
  chapters,
  bookSeconds,
  current,
  onSeek,
}: {
  chapters: Chapter[];
  bookSeconds: number;
  current?: number | null | undefined;
  onSeek?: (seconds: number) => void;
}) {
  const { theme } = useUnistyles();
  return (
    <View>
      {chapters.map((chapter, index) => {
        const playing = index === current;
        const line = (
          <View key={chapter.position} style={styles.chapter(playing)}>
            <View style={styles.marker}>
              {playing ? (
                <Icon name="volume-2" size={14} color={theme.colors.accent} />
              ) : (
                <Text variant="caption" muted>
                  {index + 1}
                </Text>
              )}
            </View>
            <Text numberOfLines={1} style={[styles.name, playing && styles.playing]}>
              {chapter.name || `Chapter ${index + 1}`}
            </Text>
            <Text variant="caption" muted style={styles.length}>
              {duration(chapterLength(chapters, index, bookSeconds))}
            </Text>
          </View>
        );
        return onSeek ? (
          <Pressable
            key={chapter.position}
            accessibilityRole="button"
            accessibilityLabel={`Play ${chapter.name || `chapter ${index + 1}`}`}
            onPress={() => onSeek(chapter.start)}
          >
            {line}
          </Pressable>
        ) : (
          line
        );
      })}
    </View>
  );
}

/** The current book's chapters in a queue, each playing from its start when chosen. */
export function QueueChapters({
  queueId,
  queue,
  run,
}: {
  queueId: string;
  queue: PlayerQueue;
  run: Run;
}) {
  const { connection } = useConnection();
  const item = queue.current_item;
  const chapters = item?.media_item?.metadata.chapters ?? [];
  if (!playsABook(queue) || chapters.length === 0) return null;
  return (
    <ChapterList
      chapters={chapters}
      bookSeconds={item?.duration ?? 0}
      current={queue.current_chapter}
      onSeek={(seconds) => run(() => seek(connection, queueId, seconds))}
    />
  );
}

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    justifyContent: 'center',
    gap: theme.spacing.sm,
  },
  chapter: (playing: boolean) => ({
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.sm,
    paddingVertical: theme.spacing.xs,
    paddingHorizontal: theme.spacing.xs,
    marginHorizontal: -theme.spacing.xs,
    borderRadius: theme.radius.md,
    backgroundColor: playing ? theme.colors.surfaceVariant : 'transparent',
  }),
  marker: { width: 24, alignItems: 'center' },
  name: { flex: 1, minWidth: 0 },
  playing: { color: theme.colors.accent, fontWeight: '600' },
  length: { fontVariant: ['tabular-nums'] },
}));
