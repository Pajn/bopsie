import { type LibraryKind, libraryItems } from '@bopsie/client/api/endpoints/library';
import type { Album, Artist, Playlist, Track } from '@bopsie/client/api/schemas/media';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { LegendList } from '@legendapp/list/react-native';
import { useInfiniteQuery } from '@tanstack/react-query';
import { useRouter } from 'expo-router';
import { useState } from 'react';
import { Pressable, TextInput, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { usePlaybackActions } from '@/features/player/usePlayback';
import { describeError } from '@/lib/errors';
import { useDebounced } from '@/lib/useDebounced';
import { Icon } from '@/ui/Icon';
import { InlineError, Screen, Text } from '@/ui/primitives';
import { SkeletonRows } from '@/ui/Skeleton';
import { AlbumRow, ArtistRow, PlaylistRow, TrackRow } from './rows';
import { useLibrarySort } from './useLibrarySort';

const KINDS: Array<{ kind: LibraryKind; label: string }> = [
  { kind: 'albums', label: 'Albums' },
  { kind: 'artists', label: 'Artists' },
  { kind: 'tracks', label: 'Tracks' },
  { kind: 'playlists', label: 'Playlists' },
];
const PAGE_SIZE = 100;

type Item = Album | Artist | Track | Playlist;

export function LibraryScreen() {
  const [kind, setKind] = useState<LibraryKind>('albums');
  const [filter, setFilter] = useState('');
  const search = useDebounced(filter.trim(), 300);
  const sort = useLibrarySort(kind);
  const { connection, account } = useConnection();
  const playback = usePlaybackActions();
  const router = useRouter();
  const { theme } = useUnistyles();
  const query = useInfiniteQuery({
    queryKey: [...queryKeys.libraryList(account.id, kind, search), sort.key],
    queryFn: ({ pageParam }) =>
      libraryItems(connection, kind, {
        limit: PAGE_SIZE,
        offset: pageParam,
        orderBy: sort.key,
        search,
      }) as Promise<Item[]>,
    initialPageParam: 0,
    getNextPageParam: (last, pages) =>
      last.length < PAGE_SIZE ? undefined : pages.length * PAGE_SIZE,
  });
  const items = query.data?.pages.flat() ?? [];

  // A track plays on through the tracks listed after it, and those before it stay behind it.
  const playTrack = (track: Track) =>
    playback.play(
      (items as Track[]).map((item) => item.uri),
      track.uri,
    );

  return (
    <Screen>
      <Text variant="title" style={styles.title}>
        Library
      </Text>
      <View style={styles.segments} accessibilityRole="tablist">
        {KINDS.map((entry) => (
          <Pressable
            key={entry.kind}
            onPress={() => setKind(entry.kind)}
            style={[styles.segment, entry.kind === kind && styles.segmentActive]}
            accessibilityRole="tab"
            accessibilityState={{ selected: entry.kind === kind }}
            testID={`library-${entry.kind}`}
          >
            <Text variant="label" style={entry.kind === kind ? styles.segmentTextActive : null}>
              {entry.label}
            </Text>
          </Pressable>
        ))}
      </View>
      <View style={styles.tools}>
        <View style={styles.filter}>
          <Icon name="search" size={16} color={theme.colors.textMuted} />
          <TextInput
            value={filter}
            onChangeText={setFilter}
            placeholder={`Filter ${KINDS.find((entry) => entry.kind === kind)?.label.toLowerCase()}`}
            placeholderTextColor={theme.colors.textMuted}
            autoCapitalize="none"
            autoCorrect={false}
            returnKeyType="search"
            style={styles.filterInput}
            testID="library-filter"
          />
          {filter ? (
            <Pressable
              onPress={() => setFilter('')}
              hitSlop={12}
              accessibilityRole="button"
              accessibilityLabel="Clear the filter"
              testID="library-filter-clear"
            >
              <Icon name="x" size={16} color={theme.colors.textMuted} />
            </Pressable>
          ) : null}
        </View>
        <Pressable
          onPress={() => router.push({ pathname: '/library-sort', params: { kind } })}
          style={({ pressed }) => [styles.sort, pressed && styles.pressed]}
          accessibilityRole="button"
          accessibilityLabel={`Sorted by ${sort.label}. Change the order`}
          testID="library-sort"
        >
          <Icon name="bar-chart-2" size={14} color={theme.colors.accent} />
          <Text variant="label" style={styles.sortText}>
            {sort.label}
          </Text>
        </Pressable>
      </View>
      {query.isError ? <InlineError message={describeError(query.error)} /> : null}
      <LegendList
        data={items}
        keyExtractor={(item) => `${item.provider}/${item.item_id}`}
        renderItem={({ item }) =>
          kind === 'albums' ? (
            <AlbumRow album={item as Album} />
          ) : kind === 'artists' ? (
            <ArtistRow artist={item as Artist} />
          ) : kind === 'tracks' ? (
            <TrackRow track={item as Track} showArtwork onPress={() => playTrack(item as Track)} />
          ) : (
            <PlaylistRow playlist={item as Playlist} />
          )
        }
        estimatedItemSize={64}
        recycleItems
        keyboardShouldPersistTaps="handled"
        keyboardDismissMode="on-drag"
        onEndReached={() => {
          if (query.hasNextPage && !query.isFetchingNextPage) void query.fetchNextPage();
        }}
        ListEmptyComponent={
          query.isPending ? (
            <SkeletonRows round={kind === 'artists'} artwork={kind === 'tracks' ? 40 : 48} />
          ) : (
            <Text muted style={styles.empty}>
              {search ? `Nothing matches “${search}”.` : 'Nothing here yet.'}
            </Text>
          )
        }
        testID="library-list"
      />
    </Screen>
  );
}

const styles = StyleSheet.create((theme) => ({
  title: { marginVertical: theme.spacing.md },
  segments: {
    flexDirection: 'row',
    gap: theme.spacing.sm,
    marginBottom: theme.spacing.md,
  },
  segment: {
    paddingHorizontal: theme.spacing.md,
    paddingVertical: theme.spacing.xs + 2,
    borderRadius: theme.radius.full,
    backgroundColor: theme.colors.surfaceVariant,
  },
  segmentActive: { backgroundColor: theme.colors.accent },
  segmentTextActive: { color: theme.colors.onAccent },
  tools: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.sm,
    marginBottom: theme.spacing.sm,
  },
  filter: {
    flex: 1,
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.sm,
    paddingHorizontal: theme.spacing.md,
    borderRadius: theme.radius.md,
    backgroundColor: theme.colors.surfaceVariant,
  },
  filterInput: {
    ...theme.typography.body,
    flex: 1,
    color: theme.colors.text,
    paddingVertical: theme.spacing.sm,
  },
  sort: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.xs,
    paddingHorizontal: theme.spacing.md,
    paddingVertical: theme.spacing.sm,
  },
  sortText: { color: theme.colors.accent },
  pressed: { opacity: 0.6 },
  empty: { marginTop: theme.spacing.xl, textAlign: 'center' },
}));
