import {
  getQueueItems,
  moveQueueItem,
  playQueueItem,
  removeQueueItem,
} from '@bopsie/client/api/endpoints/players';
import type { QueueItem } from '@bopsie/client/api/schemas/player';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useMemo } from 'react';
import { Alert } from 'react-native';
import { useConnection } from '@/connection/ConnectionProvider';
import { useActiveGroup } from '@/features/outputs/ActiveGroupProvider';
import { describeError } from '@/lib/errors';
import { moveItem } from './queue';

/** Every item in the active group's queue, in play order. */
export function useQueueItems() {
  const { connection, account } = useConnection();
  const { activeId } = useActiveGroup();
  return useQuery({
    queryKey: queryKeys.queueItems(account.id, activeId ?? 'none'),
    queryFn: () => (activeId ? getQueueItems(connection, activeId) : []),
    enabled: activeId !== null,
    // Events mark the items stale when they change.
    staleTime: Number.POSITIVE_INFINITY,
  });
}

function report(error: unknown) {
  Alert.alert('Could not change the queue', describeError(error));
}

/**
 * What the queue sheet can do to the active group's queue. Edits show at
 * once; the server's own order replaces them when the command settles, so one
 * it refuses or ignores is withdrawn.
 */
export function useQueueActions() {
  const { connection, account } = useConnection();
  const { activeId } = useActiveGroup();
  const queryClient = useQueryClient();

  return useMemo(() => {
    const key = queryKeys.queueItems(account.id, activeId ?? 'none');

    const edit = (
      change: (items: QueueItem[]) => QueueItem[],
      work: (id: string) => Promise<void>,
    ) => {
      if (!activeId) return;
      // A fetch already under way would bring back the order from before the edit.
      void queryClient.cancelQueries({ queryKey: key });
      queryClient.setQueryData<QueueItem[]>(key, (items) => items && change(items));
      work(activeId)
        .catch(report)
        .finally(() => void queryClient.invalidateQueries({ queryKey: key }));
    };

    return {
      /** Plays the queue from `item`. */
      play: (item: QueueItem) => {
        if (!activeId) return;
        playQueueItem(connection, activeId, item.queue_item_id).catch(report);
      },

      /** Moves the item at queue index `from` to index `to`. */
      move: (item: QueueItem, from: number, to: number) => {
        if (from === to) return;
        edit(
          (items) => moveItem(items, from, to),
          (id) => moveQueueItem(connection, id, item.queue_item_id, to - from),
        );
      },

      remove: (item: QueueItem) =>
        edit(
          (items) => items.filter((other) => other.queue_item_id !== item.queue_item_id),
          (id) => removeQueueItem(connection, id, item.queue_item_id),
        ),
    };
  }, [connection, account.id, queryClient, activeId]);
}
