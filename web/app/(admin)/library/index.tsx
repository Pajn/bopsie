import { type LibraryKind, libraryCount, libraryItems } from '@bopsie/client/api/endpoints/library';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { useEffect, useRef, useState } from 'react';
import { View, type ViewInstance } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { DuplicateFiles } from '@/features/library/DuplicateFiles';
import { ItemGrid } from '@/features/library/ItemGrid';
import { ItemRow } from '@/features/library/ItemRow';
import { PlayButtons, TargetPicker } from '@/features/library/PlayButtons';
import { Suggestions } from '@/features/library/Suggestions';
import { describeError } from '@/lib/errors';
import { useDebounced } from '@/lib/useDebounced';
import { SearchField, Tabs } from '@/ui/controls';
import { Button, Card, Choice, InlineError, Page, Text } from '@/ui/primitives';

type Tab = LibraryKind | 'suggestions' | 'duplicates';

const KINDS: ReadonlyArray<{ value: Tab; label: string }> = [
  { value: 'albums', label: 'Albums' },
  { value: 'artists', label: 'Artists' },
  { value: 'tracks', label: 'Tracks' },
  { value: 'playlists', label: 'Playlists' },
];

const TOOLS: ReadonlyArray<{ value: Tab; label: string }> = [
  { value: 'suggestions', label: 'Suggestions' },
  { value: 'duplicates', label: 'Duplicate files' },
];

const PAGE = 100;

/** The orders the server sorts by that make sense to browse in. */
const ORDERS = [
  { value: 'sort_name', label: 'Name' },
  { value: 'timestamp_added_desc', label: 'Recently added' },
  { value: 'year_desc', label: 'Newest' },
] as const;
type Order = (typeof ORDERS)[number]['value'];

/**
 * Calls `onVisible` when it scrolls into view: put at the foot of a list, it asks for more
 * before the reader reaches the end. The button is there for a reader who gets there first.
 */
function LoadMore({ loading, onVisible }: { loading: boolean; onVisible: () => void }) {
  const ref = useRef<ViewInstance>(null);
  const latest = useRef(onVisible);
  latest.current = onVisible;
  useEffect(() => {
    const node = ref.current as unknown as Element | null;
    if (!node || typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) latest.current();
      },
      { rootMargin: '600px' },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  return (
    <View ref={ref} style={styles.more}>
      <Button title="Show more" variant="secondary" loading={loading} onPress={onVisible} />
    </View>
  );
}

export default function LibraryScreen() {
  const { account, connection } = useConnection();
  const [tab, setTab] = useState<Tab>('albums');
  const browsing = tab !== 'suggestions' && tab !== 'duplicates';
  const kind: LibraryKind = browsing ? tab : 'albums';
  const [typed, setSearch] = useState('');
  const search = useDebounced(typed.trim());
  const [order, setOrder] = useState<Order>('sort_name');
  const items = useInfiniteQuery({
    queryKey: [...queryKeys.libraryList(account.id, kind, search), order],
    queryFn: ({ pageParam }) =>
      libraryItems(connection, kind, { limit: PAGE, offset: pageParam, search, orderBy: order }),
    initialPageParam: 0,
    getNextPageParam: (last, pages) =>
      last.length < PAGE ? undefined : pages.reduce((n, page) => n + page.length, 0),
    enabled: browsing,
  });
  const all = items.data?.pages.flat() ?? [];
  const total = useQuery({
    queryKey: [...queryKeys.library(account.id), 'count', kind],
    queryFn: () => libraryCount(connection, kind),
    enabled: browsing,
  });
  // Albums and tracks are found by their own names; whoever made them, by theirs, shown first.
  const artists = useQuery({
    queryKey: queryKeys.libraryList(account.id, 'artists', search),
    queryFn: () => libraryItems(connection, 'artists', { limit: 10, offset: 0, search }),
    enabled: browsing && search.length > 0 && kind !== 'artists',
  });
  const matchingArtists = search && kind !== 'artists' ? (artists.data ?? []) : [];

  return (
    <Page title="Library" wide>
      <Tabs tabs={KINDS} after={TOOLS} value={tab} onChange={setTab} />
      {tab === 'suggestions' ? (
        <Suggestions />
      ) : tab === 'duplicates' ? (
        <DuplicateFiles />
      ) : (
        <>
          <View style={styles.toolbar}>
            <SearchField label={`Search ${kind}`} value={typed} onChange={setSearch} />
            <Choice
              title={`Sort: ${ORDERS.find((o) => o.value === order)?.label}`}
              options={ORDERS.map((o) => ({ value: o.value, label: o.label }))}
              onChoose={(value) => setOrder(value as Order)}
            />
            <TargetPicker />
          </View>
          <Text variant="caption" muted>
            {search
              ? items.isSuccess
                ? `${all.length}${items.hasNextPage ? '+' : ''} ${kind} match “${search}”`
                : ' '
              : total.data !== undefined
                ? `${total.data.toLocaleString()} ${kind}`
                : ' '}
          </Text>
          <InlineError message={items.error ? describeError(items.error) : null} />
          {matchingArtists.length > 0 ? (
            <View style={styles.section}>
              <Text variant="heading">Artists</Text>
              <ItemGrid items={matchingArtists} />
              <Text variant="heading">
                {kind === 'tracks' ? 'Tracks' : kind === 'albums' ? 'Albums' : 'Playlists'}
              </Text>
            </View>
          ) : null}
          {items.isSuccess && all.length === 0 ? (
            <Text muted>{search ? `No ${kind} match “${search}”.` : `No ${kind} yet.`}</Text>
          ) : null}
          {kind === 'tracks' ? (
            all.length > 0 ? (
              <Card style={styles.list}>
                {all.map((item) => (
                  <ItemRow key={item.uri} item={item} actions={<PlayButtons uri={item.uri} />} />
                ))}
              </Card>
            ) : null
          ) : (
            <ItemGrid items={all} />
          )}
          {items.hasNextPage ? (
            <LoadMore
              // Watched afresh with each page: one still in view after a page asks again.
              key={all.length}
              loading={items.isFetchingNextPage}
              onVisible={() => {
                if (!items.isFetchingNextPage) void items.fetchNextPage();
              }}
            />
          ) : null}
        </>
      )}
    </Page>
  );
}

const styles = StyleSheet.create((theme) => ({
  toolbar: {
    flexDirection: 'row',
    alignItems: 'center',
    flexWrap: 'wrap',
    gap: theme.spacing.md,
  },
  list: { gap: 0, paddingVertical: theme.spacing.sm },
  more: { alignItems: 'center' },
  section: { gap: theme.spacing.sm },
}));
