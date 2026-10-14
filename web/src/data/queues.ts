import { getQueue, getQueueItems } from '@bopsie/client/api/endpoints/players';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQueries, useQuery } from '@tanstack/react-query';
import { useConnection } from '@/connection/ConnectionProvider';

/** The queues of `ids`, each kept current by queue events. */
export function useQueues(ids: string[]) {
  const { account, connection } = useConnection();
  return useQueries({
    queries: ids.map((id) => ({
      queryKey: queryKeys.queue(account.id, id),
      queryFn: () => getQueue(connection, id),
    })),
  });
}

export function useQueue(id: string) {
  const { account, connection } = useConnection();
  return useQuery({
    queryKey: queryKeys.queue(account.id, id),
    queryFn: () => getQueue(connection, id),
  });
}

export function useQueueItems(id: string) {
  const { account, connection } = useConnection();
  return useQuery({
    queryKey: queryKeys.queueItems(account.id, id),
    queryFn: () => getQueueItems(connection, id),
  });
}
