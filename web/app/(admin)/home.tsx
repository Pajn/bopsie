import {
  inProgressItems,
  type LibraryKind,
  libraryItems,
} from '@bopsie/client/api/endpoints/library';
import type { Audiobook } from '@bopsie/client/api/schemas/media';
import { useQuery } from '@tanstack/react-query';
import { type ReactNode, useState } from 'react';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { bookProgress } from '@/features/audiobooks/model';
import { greeting } from '@/features/home/greeting';
import { ItemShelf } from '@/features/library/ItemGrid';
import type { Item } from '@/features/library/ItemRow';
import { TargetPicker } from '@/features/library/PlayButtons';
import { describeError } from '@/lib/errors';
import { IconButton } from '@/ui/controls';
import { InlineError, Page, Text } from '@/ui/primitives';

/** How many items a shelf holds. */
const SHELF = 12;

/** A shelf's items, from the library in an order, kept with the rest of the library's lists. */
function useShelf(kind: Exclude<LibraryKind, 'tracks'>, orderBy: string, round = 0) {
  const { connection, account } = useConnection();
  return useQuery({
    queryKey: ['library', account.id, 'shelf', kind, orderBy, round],
    queryFn: () =>
      libraryItems(connection, kind, { limit: SHELF, offset: 0, orderBy }) as Promise<Item[]>,
  });
}

/** Who wrote a book, and how far into it listening is. */
const bookCaption = (item: Item) => {
  const book = item as Audiobook;
  return [book.authors.map((author) => author.name).join(', '), bookProgress(book)]
    .filter(Boolean)
    .join(' · ');
};

/**
 * Where the web UI opens, as the app's home: the audiobooks listening continues in, what came
 * into the library lately, the playlists and artists played most, and albums picked at random
 * to rediscover, each a shelf that scrolls sideways.
 */
export default function HomeScreen() {
  const { connection, account } = useConnection();
  const [round, setRound] = useState(0);
  const listening = useQuery({
    queryKey: ['library', account.id, 'shelf', 'in_progress'],
    queryFn: () => inProgressItems(connection, SHELF),
  });
  const added = useShelf('albums', 'timestamp_added_desc');
  // The most played first; never played, the newest.
  const playlists = useShelf('playlists', 'play_count_desc');
  const artists = useShelf('artists', 'play_count_desc');
  const random = useShelf('albums', 'random', round);
  const error = added.error ?? listening.error;

  return (
    <Page title={greeting(new Date().getHours())} wide actions={<TargetPicker />}>
      <InlineError message={error ? describeError(error) : null} />
      <Shelf title="Continue listening" items={listening.data} loading={listening.isPending}>
        <ItemShelf
          items={listening.data ?? []}
          caption={bookCaption}
          loading={listening.isPending}
        />
      </Shelf>
      <Shelf title="Recently added" items={added.data} loading={added.isPending}>
        <ItemShelf items={added.data ?? []} loading={added.isPending} />
      </Shelf>
      <Shelf title="Playlists" items={playlists.data} loading={playlists.isPending}>
        <ItemShelf items={playlists.data ?? []} loading={playlists.isPending} />
      </Shelf>
      <Shelf title="Artists" items={artists.data} loading={artists.isPending}>
        <ItemShelf items={artists.data ?? []} loading={artists.isPending} />
      </Shelf>
      <Shelf
        title="Rediscover"
        items={random.data}
        loading={random.isPending}
        action={
          <IconButton
            icon="shuffle"
            label="Pick other albums"
            variant="ghost"
            size="sm"
            onPress={() => setRound((r) => r + 1)}
          />
        }
      >
        <ItemShelf items={random.data ?? []} loading={random.isPending} />
      </Shelf>
    </Page>
  );
}

/** A titled shelf, left out when it has nothing on it, as with no book being listened to. */
function Shelf({
  title,
  items,
  loading,
  action,
  children,
}: {
  title: string;
  items: unknown[] | undefined;
  loading: boolean;
  action?: ReactNode;
  children: ReactNode;
}) {
  if (!loading && (items?.length ?? 0) === 0) return null;
  return (
    <View style={styles.shelf}>
      <View style={styles.header}>
        <Text variant="heading">{title}</Text>
        {action}
      </View>
      {children}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  shelf: { gap: theme.spacing.sm },
  header: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' },
}));
