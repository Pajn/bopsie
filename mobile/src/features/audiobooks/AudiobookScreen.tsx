import { getAudiobook } from '@bopsie/client/api/endpoints/library';
import type { Chapter } from '@bopsie/client/api/schemas/media';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { LegendList } from '@legendapp/list/react-native';
import { useQuery } from '@tanstack/react-query';
import { Stack, useRouter } from 'expo-router';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { Artwork } from '@/features/library/Artwork';
import { usePlaybackActions, useQueue } from '@/features/player/usePlayback';
import { describeError } from '@/lib/errors';
import { formatDuration } from '@/player/progress';
import { Icon } from '@/ui/Icon';
import { Button, InlineError, Text } from '@/ui/primitives';
import { usePending } from '@/ui/usePending';
import { bookProgress } from './model';

/** How long a chapter runs: to its end, the next one's start, or the end of the book. */
function chapterLength(chapters: Chapter[], index: number, bookSeconds: number): number {
  const chapter = chapters[index];
  if (!chapter) return 0;
  return Math.max(0, (chapter.end ?? chapters[index + 1]?.start ?? bookSeconds) - chapter.start);
}

/**
 * An audiobook: its cover, who wrote and reads it, how far into it listening is, and its
 * chapters. Play resumes where listening left off; while the book plays, a chapter plays from
 * its start when chosen.
 */
export function AudiobookScreen({
  id,
  provider,
  name,
}: {
  id: string;
  provider: string;
  name?: string;
}) {
  const { connection, account } = useConnection();
  const { theme } = useUnistyles();
  const router = useRouter();
  const playback = usePlaybackActions();
  const queue = useQueue().data;
  const book = useQuery({
    queryKey: queryKeys.item(account.id, 'audiobook', id, provider),
    queryFn: () => getAudiobook(connection, { itemId: id, provider }),
  });
  const data = book.data;
  const play = usePending(() => (data ? playback.play(data.uri) : undefined));
  const chapters = data?.metadata.chapters ?? [];
  // This book plays now: its chapters can be jumped to.
  const playingThis = data !== undefined && queue?.current_item?.media_item?.uri === data.uri;
  const progress = data ? bookProgress(data) : null;

  return (
    <View style={styles.screen}>
      <Stack.Screen options={{ title: data?.name ?? name ?? '' }} />
      <LegendList
        data={chapters}
        keyExtractor={(chapter) => String(chapter.position)}
        renderItem={({ item, index }) => {
          const current = playingThis && queue?.current_chapter === index;
          return (
            <Pressable
              onPress={() => (playingThis ? void playback.seek(item.start) : undefined)}
              disabled={!playingThis}
              style={({ pressed }) => [styles.chapter, pressed && styles.pressed]}
              accessibilityRole={playingThis ? 'button' : 'text'}
              accessibilityLabel={item.name || `Chapter ${index + 1}`}
              testID={`chapter-${index}`}
            >
              <View style={styles.number}>
                {current ? (
                  <Icon name="volume-2" size={16} color={theme.colors.accent} />
                ) : (
                  <Text muted>{index + 1}</Text>
                )}
              </View>
              <Text numberOfLines={1} style={[styles.chapterName, current && styles.current]}>
                {item.name || `Chapter ${index + 1}`}
              </Text>
              <Text variant="caption" muted>
                {formatDuration(chapterLength(chapters, index, data?.duration ?? 0))}
              </Text>
            </Pressable>
          );
        }}
        estimatedItemSize={48}
        ListHeaderComponent={
          <View style={styles.header}>
            <Artwork item={data ?? { image: null }} size={200} imageSize={512} placeholder="book" />
            <View style={styles.names}>
              <Text variant="heading" style={styles.center} testID="item-title">
                {data?.name ?? name ?? ''}
              </Text>
              {data && data.authors.length > 0 ? (
                <View style={styles.authors}>
                  {data.authors.map((author) => (
                    <Pressable
                      key={author.item_id}
                      onPress={() =>
                        router.push({
                          pathname: '/artist/[id]',
                          params: {
                            id: author.item_id,
                            provider: author.provider,
                            name: author.name,
                          },
                        })
                      }
                      accessibilityRole="link"
                      hitSlop={6}
                    >
                      <Text style={styles.author}>{author.name}</Text>
                    </Pressable>
                  ))}
                </View>
              ) : null}
              {data && data.narrators.length > 0 ? (
                <Text muted style={styles.center}>
                  {`Read by ${data.narrators.join(', ')}`}
                </Text>
              ) : null}
              <Text variant="caption" muted style={styles.center}>
                {[data?.duration ? formatDuration(data.duration) : null, progress]
                  .filter(Boolean)
                  .join(' · ')}
              </Text>
            </View>
            <Button
              title={data?.resume_position_ms && !data.fully_played ? 'Resume' : 'Play'}
              loading={play.pending}
              disabled={!data}
              onPress={play.onPress}
              accessibilityState={{ busy: play.pending }}
              testID="item-play"
            />
            {book.error ? <InlineError message={describeError(book.error)} /> : null}
            {data?.metadata.description ? (
              <Text muted style={styles.description}>
                {data.metadata.description}
              </Text>
            ) : null}
            {chapters.length > 0 ? (
              <Text variant="label" muted style={styles.heading}>
                {`${chapters.length} chapters`}
              </Text>
            ) : null}
          </View>
        }
        contentContainerStyle={styles.content}
        testID="audiobook-chapters"
      />
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  screen: { flex: 1, backgroundColor: theme.colors.background },
  content: { paddingHorizontal: theme.spacing.lg, paddingBottom: theme.spacing.xxl },
  header: { alignItems: 'center', gap: theme.spacing.md, paddingVertical: theme.spacing.lg },
  names: { gap: theme.spacing.xs, alignItems: 'center' },
  center: { textAlign: 'center' },
  authors: { flexDirection: 'row', flexWrap: 'wrap', justifyContent: 'center', gap: 8 },
  author: { color: theme.colors.accent, fontWeight: '600' },
  description: { alignSelf: 'stretch' },
  heading: { alignSelf: 'flex-start', marginTop: theme.spacing.md },
  chapter: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingVertical: theme.spacing.sm,
  },
  number: { width: 28, alignItems: 'center' },
  chapterName: { flex: 1 },
  current: { color: theme.colors.accent, fontWeight: '600' },
  pressed: { opacity: 0.6 },
}));
