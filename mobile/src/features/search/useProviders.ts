import { listProviders } from '@bopsie/client/api/endpoints/library';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useConnection } from '@/connection/ConnectionProvider';

/** The server's music providers, such as Spotify once it is signed in. */
export function useProviders() {
  const { connection, account } = useConnection();
  return useQuery({
    queryKey: queryKeys.providers(account.id),
    queryFn: () => listProviders(connection),
    staleTime: 60_000,
  });
}
