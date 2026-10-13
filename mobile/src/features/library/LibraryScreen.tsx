import { LegendList } from '@legendapp/list/react-native';
import { useInfiniteQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { ActivityIndicator, Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { type LibraryKind, libraryItems } from '@/api/endpoints/library';
import type { Album, Artist, Playlist } from '@/api/schemas/media';
import { useConnection } from '@/connection/ConnectionProvider';
import { queryKeys } from '@/connection/queryKeys';
import { describeError } from '@/lib/errors';
import { InlineError, Screen, Text } from '@/ui/primitives';
import { AlbumRow, ArtistRow, PlaylistRow } from './rows';

type Kind = Exclude<LibraryKind, 'tracks'>;
const KINDS: Array<{ kind: Kind; label: string }> = [
  { kind: 'albums', label: 'Albums' },
  { kind: 'artists', label: 'Artists' },
  { kind: 'playlists', label: 'Playlists' },
];
const PAGE_SIZE = 100;

type Item = Album | Artist | Playlist;

export function LibraryScreen() {
  const [kind, setKind] = useState<Kind>('albums');
  const { connection, account } = useConnection();
  const { theme } = useUnistyles();
  const query = useInfiniteQuery({
    queryKey: queryKeys.libraryList(account.id, kind, ''),
    queryFn: ({ pageParam }) =>
      libraryItems(connection, kind, { limit: PAGE_SIZE, offset: pageParam }) as Promise<Item[]>,
    initialPageParam: 0,
    getNextPageParam: (last, pages) =>
      last.length < PAGE_SIZE ? undefined : pages.length * PAGE_SIZE,
  });
  const items = query.data?.pages.flat() ?? [];

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
      {query.isError ? <InlineError message={describeError(query.error)} /> : null}
      <LegendList
        data={items}
        keyExtractor={(item) => `${item.provider}/${item.item_id}`}
        renderItem={({ item }) =>
          kind === 'albums' ? (
            <AlbumRow album={item as Album} />
          ) : kind === 'artists' ? (
            <ArtistRow artist={item as Artist} />
          ) : (
            <PlaylistRow playlist={item as Playlist} />
          )
        }
        estimatedItemSize={64}
        recycleItems
        onEndReached={() => {
          if (query.hasNextPage && !query.isFetchingNextPage) void query.fetchNextPage();
        }}
        ListEmptyComponent={
          query.isPending ? (
            <ActivityIndicator color={theme.colors.accent} style={styles.empty} />
          ) : (
            <Text muted style={styles.empty}>
              Nothing here yet.
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
  empty: { marginTop: theme.spacing.xl, textAlign: 'center' },
}));
