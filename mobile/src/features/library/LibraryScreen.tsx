import {
  audiobooks,
  type LibraryKind,
  libraryItems,
  searchAudiobooks,
} from '@bopsie/client/api/endpoints/library';
import type { Album, Artist, Audiobook, Playlist, Track } from '@bopsie/client/api/schemas/media';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { LegendList } from '@legendapp/list/react-native';
import { useInfiniteQuery } from '@tanstack/react-query';
import { useRouter } from 'expo-router';
import { useState } from 'react';
import { Pressable, ScrollView, TextInput, useWindowDimensions, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { usePlaybackActions } from '@/features/player/usePlayback';
import { describeError } from '@/lib/errors';
import { useDebounced } from '@/lib/useDebounced';
import { Icon } from '@/ui/Icon';
import { InlineError, Screen, Text } from '@/ui/primitives';
import { SkeletonRows } from '@/ui/Skeleton';
import { AlbumRow, ArtistRow, BookRow, PlaylistRow, TrackRow } from './rows';
import { chooseLibraryLayout, useLibraryLayout, useLibrarySort } from './useLibrarySort';

/** The library's kinds, and the audiobooks the sources serve, which the library stores none of. */
type Kind = LibraryKind | 'audiobooks';

const KINDS: Array<{ kind: Kind; label: string }> = [
  { kind: 'albums', label: 'Albums' },
  { kind: 'artists', label: 'Artists' },
  { kind: 'tracks', label: 'Tracks' },
  { kind: 'playlists', label: 'Playlists' },
  { kind: 'audiobooks', label: 'Audiobooks' },
];
const PAGE_SIZE = 100;

type Item = Album | Artist | Track | Playlist | Audiobook;

export function LibraryScreen() {
  const [kind, setKind] = useState<Kind>('albums');
  const books = kind === 'audiobooks';
  // Books keep the source's order and lay out as albums do.
  const musicKind: LibraryKind = books ? 'albums' : kind;
  const { theme } = useUnistyles();
  const [filter, setFilter] = useState('');
  const search = useDebounced(filter.trim(), 300);
  const sort = useLibrarySort(musicKind);
  const layout = useLibraryLayout(musicKind);
  const grid = layout === 'grid';
  const { width } = useWindowDimensions();
  // Two across, between the screen's padding and a gap.
  const tile = Math.floor((width - theme.spacing.lg * 2 - theme.spacing.md) / 2);
  const { connection, account } = useConnection();
  const playback = usePlaybackActions();
  const router = useRouter();
  const query = useInfiniteQuery({
    queryKey: [...queryKeys.libraryList(account.id, kind, search), books ? '' : sort.key],
    queryFn: ({ pageParam }): Promise<Item[]> =>
      books
        ? search
          ? searchAudiobooks(connection, search)
          : audiobooks(connection, { limit: PAGE_SIZE, offset: pageParam })
        : libraryItems(connection, musicKind, {
            limit: PAGE_SIZE,
            offset: pageParam,
            orderBy: sort.key,
            search,
          }),
    initialPageParam: 0,
    getNextPageParam: (last, pages) =>
      (books && search) || last.length < PAGE_SIZE ? undefined : pages.length * PAGE_SIZE,
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
      <ScrollView
        horizontal
        showsHorizontalScrollIndicator={false}
        style={styles.segmentScroll}
        contentContainerStyle={styles.segments}
        accessibilityRole="tablist"
      >
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
      </ScrollView>
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
        {books ? null : (
          <Pressable
            onPress={() => router.push({ pathname: '/library-sort', params: { kind: musicKind } })}
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
        )}
        {kind !== 'tracks' ? (
          <Pressable
            onPress={() => chooseLibraryLayout(musicKind, grid ? 'list' : 'grid')}
            hitSlop={8}
            style={({ pressed }) => [styles.layout, pressed && styles.pressed]}
            accessibilityRole="button"
            accessibilityLabel={grid ? 'Show as a list' : 'Show as a grid'}
            testID="library-layout"
          >
            <Icon name={grid ? 'list' : 'grid'} size={18} color={theme.colors.accent} />
          </Pressable>
        ) : null}
      </View>
      {query.isError ? <InlineError message={describeError(query.error)} /> : null}
      <LegendList
        data={items}
        keyExtractor={(item) => `${item.provider}/${item.item_id}`}
        renderItem={({ item, index }) => (
          // A tile holds to the outer edge of its column, leaving the gap between the two.
          <View style={grid && (index % 2 === 0 ? styles.left : styles.right)}>
            {kind === 'albums' ? (
              <AlbumRow album={item as Album} {...(grid && { tile })} />
            ) : kind === 'artists' ? (
              <ArtistRow artist={item as Artist} {...(grid && { tile })} />
            ) : kind === 'tracks' ? (
              <TrackRow
                track={item as Track}
                showArtwork
                onPress={() => playTrack(item as Track)}
              />
            ) : books ? (
              <BookRow book={item as Audiobook} {...(grid && { tile })} />
            ) : (
              <PlaylistRow playlist={item as Playlist} {...(grid && { tile })} />
            )}
          </View>
        )}
        key={grid ? 'grid' : 'list'}
        numColumns={grid ? 2 : 1}
        estimatedItemSize={grid ? tile + 56 : 64}
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
              {search
                ? `Nothing matches “${search}”.`
                : books
                  ? 'No audiobooks. An admin signs the server in to Audiobookshelf in its web UI.'
                  : 'Nothing here yet.'}
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
  segmentScroll: { flexGrow: 0, marginBottom: theme.spacing.md },
  segments: { flexDirection: 'row', gap: theme.spacing.sm },
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
  left: { alignItems: 'flex-start' },
  right: { alignItems: 'flex-end' },
  layout: { paddingVertical: theme.spacing.sm, paddingRight: theme.spacing.xs },
  pressed: { opacity: 0.6 },
  empty: { marginTop: theme.spacing.xl, textAlign: 'center' },
}));
