import { separateAlbum } from '@bopsie/client/api/endpoints/admin';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQueryClient } from '@tanstack/react-query';
import { useRouter } from 'expo-router';
import { useState } from 'react';
import { useConnection } from '@/connection/ConnectionProvider';
import { describeError } from '@/lib/errors';
import { Badge, Button, Card, Facts, InlineError, Stack, Text } from '@/ui/primitives';
import type { Item } from './ItemRow';

/**
 * The providers an item comes from, the one it plays from first at the top. An album merged
 * from several can have one taken back out into an album of its own, which then stays apart.
 */
export function Sources({ item }: { item: Item }) {
  const { account, connection } = useConnection();
  const queryClient = useQueryClient();
  const router = useRouter();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mappings = item.provider_mappings;
  const instances = new Set(mappings.map((mapping) => mapping.provider_instance));
  const separable = item.media_type === 'album' && instances.size > 1;

  const separate = async (instance: string) => {
    setError(null);
    setBusy(instance);
    try {
      const { item_id } = await separateAlbum(connection, item.item_id, instance);
      await queryClient.invalidateQueries({ queryKey: queryKeys.library(account.id) });
      if (item_id) router.push(`/library/album/library/${encodeURIComponent(item_id)}`);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card title="Sources">
      {separable ? (
        <Text variant="caption" muted>
          Merged from several sources. Separate takes one back out, with the tracks only it holds,
          into an album of its own that is not merged again.
        </Text>
      ) : null}
      <InlineError message={error} />
      {mappings.map((mapping, index) => (
        <Stack key={`${mapping.provider_instance}/${mapping.item_id}`} gap="xs">
          <Stack horizontal gap="xs">
            <Text variant="label">{mapping.provider_domain}</Text>
            {index === 0 && mappings.length > 1 ? (
              <Badge label="plays first" tone="accent" />
            ) : null}
          </Stack>
          <Facts
            rows={[
              ['Provider', mapping.provider_instance],
              ['Id', mapping.item_id],
            ]}
          />
          {separable ? (
            <Button
              title="Separate"
              variant="secondary"
              loading={busy === mapping.provider_instance}
              disabled={busy !== null}
              onPress={() => void separate(mapping.provider_instance)}
            />
          ) : null}
        </Stack>
      ))}
    </Card>
  );
}
