import { search } from '@bopsie/client/api/endpoints/library';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useRouter } from 'expo-router';
import { type ReactNode, useState } from 'react';
import { ActivityIndicator, Pressable, ScrollView, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { Artwork } from '@/features/library/Artwork';
import { AlbumRow, ArtistRow, artistNames, PlaylistRow, TrackRow } from '@/features/library/rows';
import { usePlaybackActions } from '@/features/player/usePlayback';
import { describeError } from '@/lib/errors';
import { useDebounced } from '@/lib/useDebounced';
import { Icon } from '@/ui/Icon';
import { InlineError, Screen, Stack, Text, TextField } from '@/ui/primitives';
import { SkeletonRows } from '@/ui/Skeleton';
import { usePending } from '@/ui/usePending';
import { EVERYWHERE, type SearchScope, searchScopes } from './scope';
import { type TopResult, topResult } from './searchResults';
import { useProviders } from './useProviders';
import { useRecentSearches } from './useRecentSearches';

export function SearchScreen() {
  const { connection, account } = useConnection();
  const playback = usePlaybackActions();
  const [text, setText] = useState('');
  const query = useDebounced(text.trim(), 300);
  const scopes = searchScopes(useProviders().data ?? []);
  // Kept while the tab is, so the next search looks where the last one did.
  const [scopeKey, setScopeKey] = useState(EVERYWHERE.key);
  const scope: SearchScope = scopes.find((s) => s.key === scopeKey) ?? EVERYWHERE;
  const results = useQuery({
    queryKey: queryKeys.search(account.id, query, scope.key),
    queryFn: () => search(connection, query, 25, scope.providers),
    enabled: query.length > 0,
  });
  const data = query.length > 0 ? results.data : undefined;
  const top = data ? topResult(data, query) : null;
  const recents = useRecentSearches();

  return (
    <Screen>
      <Text variant="title" style={styles.title}>
        Search
      </Text>
      <TextField
        label="Artists, albums, tracks and playlists"
        value={text}
        onChangeText={setText}
        autoCapitalize="none"
        autoCorrect={false}
        returnKeyType="search"
        onSubmitEditing={() => recents.add(text)}
        testID="search-input"
      />
      {scopes.length > 0 ? (
        <View style={styles.scopes} accessibilityRole="tablist">
          {scopes.map((option) => {
            const selected = option.key === scope.key;
            return (
              <Pressable
                key={option.key}
                onPress={() => setScopeKey(option.key)}
                style={[styles.scope, selected && styles.scopeSelected]}
                accessibilityRole="tab"
                accessibilityState={{ selected }}
                testID={`search-scope-${option.key}`}
              >
                <Text variant="caption" style={selected ? styles.scopeSelectedText : undefined}>
                  {option.label}
                </Text>
              </Pressable>
            );
          })}
        </View>
      ) : null}
      <ScrollView
        keyboardShouldPersistTaps="handled"
        keyboardDismissMode="on-drag"
        contentContainerStyle={styles.results}
        // A search counts as recent once anything in its results is touched.
        onTouchStart={() => data && recents.add(query)}
      >
        {query.length === 0 ? (
          <Recent
            recent={recents.recent}
            onSearch={setText}
            onRemove={recents.remove}
            onClear={recents.clear}
          />
        ) : null}
        {query.length > 0 && results.isFetching && !data ? <SkeletonRows count={6} /> : null}
        {results.isError ? <InlineError message={describeError(results.error)} /> : null}
        {data && !top ? (
          <Text muted style={styles.nothing}>
            {`Nothing found for “${query}”.`}
          </Text>
        ) : null}
        {data ? (
          <Stack gap="lg">
            {top ? <TopResultCard result={top} /> : null}
            <Section title="Artists" items={data.artists} keyOf={(a) => a.uri}>
              {(artist) => <ArtistRow artist={artist} />}
            </Section>
            <Section title="Albums" items={data.albums} keyOf={(a) => a.uri}>
              {(album) => <AlbumRow album={album} />}
            </Section>
            <Section title="Tracks" items={data.tracks} keyOf={(t) => t.uri}>
              {(track) => (
                <TrackRow track={track} showArtwork onPress={() => playback.play(track.uri)} />
              )}
            </Section>
            <Section title="Playlists" items={data.playlists} keyOf={(p) => p.uri}>
              {(playlist) => <PlaylistRow playlist={playlist} />}
            </Section>
          </Stack>
        ) : null}
      </ScrollView>
    </Screen>
  );
}

/** Rows shown before a section's "See all". */
const SECTION_ROWS = 4;

/** A kind of result: its first few, and the rest on request. Nothing when there are none. */
function Section<T>({
  title,
  items,
  keyOf,
  children,
}: {
  title: string;
  items: T[];
  keyOf: (item: T) => string;
  children: (item: T) => ReactNode;
}) {
  const [all, setAll] = useState(false);
  if (items.length === 0) return null;
  const shown = all ? items : items.slice(0, SECTION_ROWS);
  return (
    <View>
      <View style={styles.sectionHeader}>
        <Text variant="label" muted style={styles.sectionTitle}>
          {title}
        </Text>
        {items.length > SECTION_ROWS ? (
          <Pressable
            onPress={() => setAll(!all)}
            hitSlop={12}
            accessibilityRole="button"
            testID={`search-all-${title.toLowerCase()}`}
          >
            <Text variant="label" style={styles.action}>
              {all ? 'Show fewer' : `See all ${items.length}`}
            </Text>
          </Pressable>
        ) : null}
      </View>
      {shown.map((item) => (
        <View key={keyOf(item)}>{children(item)}</View>
      ))}
    </View>
  );
}

/** Searches made before, to make again or forget, while nothing is typed. */
function Recent({
  recent,
  onSearch,
  onRemove,
  onClear,
}: {
  recent: string[];
  onSearch: (query: string) => void;
  onRemove: (query: string) => void;
  onClear: () => void;
}) {
  const { theme } = useUnistyles();
  if (recent.length === 0) {
    return (
      <Text muted style={styles.nothing}>
        Search your library, and Spotify where it is signed in.
      </Text>
    );
  }
  return (
    <View testID="search-recent">
      <View style={styles.sectionHeader}>
        <Text variant="label" muted style={styles.sectionTitle}>
          Recent searches
        </Text>
        <Pressable onPress={onClear} hitSlop={12} accessibilityRole="button">
          <Text variant="label" style={styles.action}>
            Clear
          </Text>
        </Pressable>
      </View>
      {recent.map((query) => (
        <Pressable
          key={query}
          onPress={() => onSearch(query)}
          style={({ pressed }) => [styles.recent, pressed && styles.pressed]}
          accessibilityRole="button"
          accessibilityLabel={`Search for ${query}`}
        >
          <Icon name="clock" size={18} color={theme.colors.textMuted} />
          <Text style={styles.grow} numberOfLines={1}>
            {query}
          </Text>
          <Pressable
            onPress={() => onRemove(query)}
            hitSlop={12}
            accessibilityRole="button"
            accessibilityLabel={`Forget ${query}`}
          >
            <Icon name="x" size={16} color={theme.colors.textMuted} />
          </Pressable>
        </Pressable>
      ))}
    </View>
  );
}

const KINDS = { artist: 'Artist', album: 'Album', track: 'Track', playlist: 'Playlist' } as const;

/**
 * The result the search most likely meant, large, with a way to play it at once, which spins
 * until it has started.
 */
function TopResultCard({ result }: { result: TopResult }) {
  const router = useRouter();
  const playback = usePlaybackActions();
  const { theme } = useUnistyles();
  const { item, kind } = result;
  const play = usePending(() => playback.play(item.uri));
  const open = () => {
    if (kind === 'track') {
      play.onPress();
      return;
    }
    const pathname =
      kind === 'album' ? '/album/[id]' : kind === 'artist' ? '/artist/[id]' : '/playlist/[id]';
    router.push({
      pathname,
      params: { id: item.item_id, provider: item.provider, name: item.name },
    });
  };
  const by = result.kind === 'album' || result.kind === 'track' ? artistNames(result.item) : '';
  return (
    <Pressable
      onPress={open}
      style={({ pressed }) => [styles.top, pressed && styles.pressed]}
      accessibilityRole="button"
      accessibilityLabel={`Top result: ${item.name}`}
      accessibilityState={{ busy: kind === 'track' && play.pending }}
      testID="search-top"
    >
      <Artwork
        item={item}
        size={88}
        imageSize={256}
        {...(kind === 'artist' && { round: true, placeholder: 'user' as const })}
        {...(kind === 'playlist' && { placeholder: 'list' as const })}
      />
      <View style={styles.grow}>
        <Text variant="heading" numberOfLines={2}>
          {item.name}
        </Text>
        <Text variant="caption" muted numberOfLines={1}>
          {[KINDS[kind], by].filter(Boolean).join(' · ')}
        </Text>
      </View>
      <Pressable
        onPress={play.onPress}
        hitSlop={8}
        style={({ pressed }) => [styles.play, pressed && styles.pressed]}
        accessibilityRole="button"
        accessibilityLabel={`Play ${item.name}`}
        accessibilityState={{ busy: play.pending }}
        testID="search-top-play"
      >
        {play.pending ? (
          <ActivityIndicator color={theme.colors.onAccent} />
        ) : (
          <Icon name="play" size={22} color={theme.colors.onAccent} style={styles.playIcon} />
        )}
      </Pressable>
    </Pressable>
  );
}

const styles = StyleSheet.create((theme) => ({
  title: { marginVertical: theme.spacing.md },
  scopes: { flexDirection: 'row', gap: theme.spacing.sm, marginTop: theme.spacing.md },
  scope: {
    paddingHorizontal: theme.spacing.md,
    paddingVertical: theme.spacing.xs,
    borderRadius: 999,
    borderWidth: 1,
    borderColor: theme.colors.border,
  },
  scopeSelected: { backgroundColor: theme.colors.accent, borderColor: theme.colors.accent },
  scopeSelectedText: { color: theme.colors.onAccent },
  results: { paddingVertical: theme.spacing.lg },
  nothing: { textAlign: 'center', marginTop: theme.spacing.lg },
  sectionHeader: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    marginBottom: theme.spacing.xs,
  },
  sectionTitle: { letterSpacing: 0.8, textTransform: 'uppercase' },
  action: { color: theme.colors.accent },
  recent: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingVertical: theme.spacing.md,
  },
  grow: { flex: 1, gap: 2 },
  pressed: { opacity: 0.6 },
  top: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    padding: theme.spacing.md,
    borderRadius: theme.radius.md,
    backgroundColor: theme.colors.surfaceVariant,
  },
  play: {
    width: 48,
    height: 48,
    borderRadius: 24,
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor: theme.colors.accent,
  },
  // A triangle's weight sits left of its box; nudged right, it looks centred.
  playIcon: { marginLeft: 3 },
}));
