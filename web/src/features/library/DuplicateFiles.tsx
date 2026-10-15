import { duplicateFiles } from '@bopsie/client/api/endpoints/admin';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { describeError } from '@/lib/errors';
import { Card, InlineError, Stack, Text } from '@/ui/primitives';
import { ItemRow } from './ItemRow';

/**
 * Tracks kept as more than one file, such as an album's and a copy in a playlist's folder. The
 * library shows each once and plays either; this lists them so the copies not wanted can be
 * deleted from disk. The server never removes files.
 */
export function DuplicateFiles() {
  const { account, connection } = useConnection();
  const duplicates = useQuery({
    queryKey: queryKeys.admin(account.id, 'duplicate-files'),
    queryFn: () => duplicateFiles(connection),
  });
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
      {duplicates.data?.length ? (
        <Card title={`${duplicates.data.length} tracks`}>
          {duplicates.data.map(({ track, files }) => (
            <View key={track.uri} style={styles.entry}>
              <ItemRow item={track} />
              {files.map((file) => (
                <Text key={file} variant="caption" selectable style={styles.file}>
                  {file}
                </Text>
              ))}
            </View>
          ))}
        </Card>
      ) : null}
    </Stack>
  );
}

const styles = StyleSheet.create((theme) => ({
  entry: { gap: theme.spacing.xs, paddingBottom: theme.spacing.sm },
  file: { fontFamily: 'monospace' },
}));
