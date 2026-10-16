import {
  type LibraryKindName,
  libraryItemEdits,
  updateLibraryItem,
} from '@bopsie/client/api/endpoints/admin';
import { imageUrl } from '@bopsie/client/api/images';
import type { Album, Track } from '@bopsie/client/api/schemas/media';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Image, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import ArtworkPicker from '@/debug/ArtworkPicker';
import { describeError } from '@/lib/errors';
import { Badge, Button, InlineError, Stack, Text, TextField } from '@/ui/primitives';
import type { Item } from './ItemRow';

/** The fields of each kind, as the edit form shows them. */
const FIELDS: Record<LibraryKindName, string[]> = {
  track: ['name', 'sort_name', 'track_number', 'disc_number', 'album', 'artists', 'image'],
  album: ['name', 'sort_name', 'year', 'artists', 'image'],
  artist: ['name', 'sort_name'],
  playlist: ['name'],
};

const LABELS: Record<string, string> = {
  name: 'Name',
  sort_name: 'Sort name',
  track_number: 'Track number',
  disc_number: 'Disc number',
  year: 'Year',
  album: 'Album',
  artists: 'Artists',
  image: 'Artwork',
};

const NUMBERS = new Set(['track_number', 'disc_number', 'year']);

/** Names, comma separated. */
const listed = (text: string) =>
  text
    .split(',')
    .map((name) => name.trim())
    .filter(Boolean);

/** What the form shows in a field now. */
function current(item: Item, field: string): string {
  const any = item as unknown as Record<string, unknown>;
  switch (field) {
    case 'artists':
      return ((item as Track | Album).artists ?? []).map((a) => a.name).join(', ');
    case 'album':
      return (item as Track).album?.name ?? '';
    case 'image':
      return '';
    default: {
      const value = any[field];
      return value === null || value === undefined ? '' : String(value);
    }
  }
}

/** What a field's text is saved as. */
function changeOf(field: string, text: string): unknown {
  if (NUMBERS.has(field)) return Number(text);
  if (field === 'artists') return listed(text).map((name) => ({ name }));
  if (field === 'album') return { name: text.trim() };
  return text;
}

/**
 * Edits an item's metadata. Edits are kept apart from what the scan finds and outlast a rescan
 * of its file; Reset brings back what the scan found.
 */
export function EditItem({ item, kind }: { item: Item; kind: LibraryKindName }) {
  const { account, connection, token } = useConnection();
  const { theme } = useUnistyles();
  const queryClient = useQueryClient();
  const editsKey = ['admin', account.id, 'edits', kind, item.item_id] as const;
  const edits = useQuery({
    queryKey: editsKey,
    queryFn: () => libraryItemEdits(connection, kind, item.item_id),
  });
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const fields = FIELDS[kind];
  const value = (field: string) => drafts[field] ?? current(item, field);
  const changed = fields.filter(
    (field) => drafts[field] !== undefined && drafts[field] !== current(item, field),
  );

  const save = async (changes: Record<string, unknown>) => {
    setError(null);
    setBusy(true);
    try {
      await updateLibraryItem(connection, kind, item.item_id, changes);
      setDrafts({});
      await queryClient.invalidateQueries({ queryKey: editsKey });
      await queryClient.invalidateQueries({ queryKey: queryKeys.library(account.id) });
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(false);
    }
  };

  const artwork = imageUrl(account.serverUrl, item, 160);

  return (
    <Stack gap="md">
      <Text variant="caption" muted>
        Edits outlast a rescan of the file. Reset brings back what the scan found
        {kind === 'track' || kind === 'playlist' ? ', reading the file again' : ''}.
        {kind === 'track' ? ' An album or artist that does not exist yet is made.' : ''}
      </Text>
      {fields
        .filter((field) => field !== 'image')
        .map((field) => (
          <View key={field} style={styles.row}>
            <View style={styles.label}>
              <Text variant="label">{LABELS[field]}</Text>
              {field === 'artists' ? (
                <Text variant="caption" muted>
                  Comma separated
                </Text>
              ) : null}
            </View>
            <TextField
              label=""
              value={value(field)}
              keyboardType={NUMBERS.has(field) ? 'number-pad' : 'default'}
              onChangeText={(text) => setDrafts({ ...drafts, [field]: text })}
              style={styles.input}
            />
            {edits.data && field in edits.data ? (
              <Stack horizontal gap="xs">
                <Badge label="edited" tone="accent" />
                <Button
                  title="Reset"
                  variant="secondary"
                  onPress={() => void save({ [field]: null })}
                />
              </Stack>
            ) : null}
          </View>
        ))}
      {fields.includes('image') ? (
        <View style={styles.row}>
          <View style={styles.label}>
            <Text variant="label">{LABELS.image}</Text>
          </View>
          <Stack horizontal gap="md" style={styles.input}>
            {artwork ? <Image source={{ uri: artwork }} style={styles.art} /> : null}
            <ArtworkPicker
              serverUrl={account.serverUrl}
              token={token}
              color={theme.colors.text}
              onUploaded={(proxyId) => void save({ image: proxyId })}
              onFailed={setError}
            />
          </Stack>
          {edits.data && 'image' in edits.data ? (
            <Stack horizontal gap="xs">
              <Badge label="edited" tone="accent" />
              <Button
                title="Reset"
                variant="secondary"
                onPress={() => void save({ image: null })}
              />
            </Stack>
          ) : null}
        </View>
      ) : null}
      <InlineError message={error} />
      <Stack horizontal gap="xs">
        <Button
          title="Save"
          loading={busy}
          disabled={changed.length === 0}
          onPress={() =>
            void save(
              Object.fromEntries(
                changed.map((field) => [field, changeOf(field, drafts[field] ?? '')]),
              ),
            )
          }
        />
        <Button
          title="Discard"
          variant="secondary"
          disabled={changed.length === 0}
          onPress={() => setDrafts({})}
        />
      </Stack>
    </Stack>
  );
}

const styles = StyleSheet.create((theme) => ({
  // The label above its field: the form sits in a narrow column beside the item's contents.
  row: { gap: theme.spacing.xs },
  label: { flexDirection: 'row', alignItems: 'baseline', gap: theme.spacing.sm },
  input: { flex: 1 },
  art: { width: 80, height: 80, borderRadius: theme.radius.sm },
}));
