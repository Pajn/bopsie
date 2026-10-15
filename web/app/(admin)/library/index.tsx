import { type LibraryKind, libraryItems } from '@bopsie/client/api/endpoints/library';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useInfiniteQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { ScrollView } from 'react-native';
import { useConnection } from '@/connection/ConnectionProvider';
import { ItemRow } from '@/features/library/ItemRow';
import { PlayButtons, TargetPicker } from '@/features/library/PlayButtons';
import { Suggestions } from '@/features/library/Suggestions';
import { describeError } from '@/lib/errors';
import { Button, Card, InlineError, Stack, Text, TextField } from '@/ui/primitives';

const KINDS: Array<{ kind: LibraryKind; title: string }> = [
  { kind: 'albums', title: 'Albums' },
  { kind: 'artists', title: 'Artists' },
  { kind: 'tracks', title: 'Tracks' },
  { kind: 'playlists', title: 'Playlists' },
];

const PAGE = 100;

export default function LibraryScreen() {
  const { account, connection } = useConnection();
  const [tab, setTab] = useState<LibraryKind | 'suggestions'>('albums');
  const kind = tab === 'suggestions' ? 'albums' : tab;
  const [search, setSearch] = useState('');
  const items = useInfiniteQuery({
    queryKey: queryKeys.libraryList(account.id, kind, search),
    queryFn: ({ pageParam }) =>
      libraryItems(connection, kind, { limit: PAGE, offset: pageParam, search }),
    initialPageParam: 0,
    getNextPageParam: (last, pages) =>
      last.length < PAGE ? undefined : pages.reduce((n, page) => n + page.length, 0),
    enabled: tab !== 'suggestions',
  });
  const all = items.data?.pages.flat() ?? [];

  return (
    <ScrollView>
      <Stack gap="lg">
        <Text variant="title">Library</Text>
        <TargetPicker />
        <Stack horizontal gap="xs">
          {KINDS.map((k) => (
            <Button
              key={k.kind}
              title={k.title}
              variant={k.kind === tab ? 'primary' : 'secondary'}
              onPress={() => setTab(k.kind)}
            />
          ))}
          <Button
            title="Suggestions"
            variant={tab === 'suggestions' ? 'primary' : 'secondary'}
            onPress={() => setTab('suggestions')}
          />
        </Stack>
        {tab === 'suggestions' ? (
          <Suggestions />
        ) : (
          <>
            <TextField label="Search" value={search} onChangeText={setSearch} autoCorrect={false} />
            <InlineError message={items.error ? describeError(items.error) : null} />
            <Card>
              {items.isSuccess && all.length === 0 ? <Text muted>Nothing here.</Text> : null}
              {all.map((item) => (
                <ItemRow key={item.uri} item={item} actions={<PlayButtons uri={item.uri} />} />
              ))}
              {items.hasNextPage ? (
                <Button
                  title="More"
                  variant="secondary"
                  loading={items.isFetchingNextPage}
                  onPress={() => void items.fetchNextPage()}
                />
              ) : null}
            </Card>
          </>
        )}
      </Stack>
    </ScrollView>
  );
}
