import { duplicateFiles } from '@bopsie/client/api/endpoints/admin';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { describeError } from '@/lib/errors';
import { Icon } from '@/ui/Icon';
import { Card, InlineError, Stack, Text } from '@/ui/primitives';
import { byFolders } from './duplicates';
import { ItemRow } from './ItemRow';

/**
 * Tracks kept as more than one file, such as an album's and a copy in a playlist's folder. The
 * library shows each once and plays either; this lists them so the copies not wanted can be
 * deleted from disk. The server never removes files.
 */
export function DuplicateFiles() {
  const { account, connection } = useConnection();
  const { theme } = useUnistyles();
  // Groups opened to show each track and its files, by their folders.
  const [open, setOpen] = useState<Set<string>>(new Set());
  const duplicates = useQuery({
    queryKey: queryKeys.admin(account.id, 'duplicate-files'),
    queryFn: () => duplicateFiles(connection),
  });
  const groups = byFolders(duplicates.data ?? []);
  return (
    <Stack gap="md">
      <Text variant="caption" muted>
        Tracks kept as more than one file. Each shows once in the library and plays from either;
        delete the copies you do not want from disk, and the next sync forgets them.
      </Text>
      <InlineError message={duplicates.error ? describeError(duplicates.error) : null} />
      {duplicates.isSuccess && duplicates.data.length === 0 ? (
        <Card>
          <Text muted>No track is kept twice.</Text>
        </Card>
      ) : null}
      {groups.length > 0 ? (
        <Text variant="label" muted>
          {duplicates.data?.length} TRACKS IN {groups.length}{' '}
          {groups.length === 1 ? 'PLACE' : 'PLACES'}
        </Text>
      ) : null}
      {groups.map((group) => {
        const key = group.folders.join('\n');
        // A few tracks are listed with their files; many, by name until asked for the rest.
        const listed = open.has(key) || group.tracks.length <= 6;
        return (
          <Card key={key}>
            <Text variant="heading">
              {group.tracks.length} {group.tracks.length === 1 ? 'track' : 'tracks'}
              {group.folders.length > 1
                ? ` in each of ${group.folders.length} folders`
                : ' twice in one folder'}
            </Text>
            <View style={styles.folders}>
              {group.folders.map((folder) => (
                <View key={folder} style={styles.folder}>
                  <Icon name="folder" size={14} color={theme.colors.textMuted} />
                  <Text variant="caption" selectable style={styles.path}>
                    {folder}
                  </Text>
                </View>
              ))}
            </View>
            {listed ? null : (
              <Text muted numberOfLines={2}>
                {group.tracks.map(({ track }) => track.name).join(' · ')}
              </Text>
            )}
            {listed ? null : (
              <Pressable
                accessibilityRole="button"
                onPress={() => setOpen(new Set([...open, key]))}
              >
                <Text variant="caption" style={styles.more}>
                  Show every track and its files
                </Text>
              </Pressable>
            )}
            {listed
              ? group.tracks.map(({ track, files }) => (
                  <View key={track.uri} style={styles.entry}>
                    <ItemRow item={track} />
                    {files.map((file) => (
                      <Text key={file} variant="caption" selectable style={styles.path}>
                        {file}
                      </Text>
                    ))}
                  </View>
                ))
              : null}
          </Card>
        );
      })}
    </Stack>
  );
}

const styles = StyleSheet.create((theme) => ({
  entry: { gap: theme.spacing.xs, paddingBottom: theme.spacing.sm },
  folders: { gap: theme.spacing.xs },
  folder: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.sm },
  path: { fontFamily: 'monospace', flex: 1, minWidth: 0 },
  more: { color: theme.colors.accent, fontWeight: '600' },
}));
