import { adminDevices, adminGroups, adminServerInfo } from '@bopsie/client/api/endpoints/admin';
import { listPlayers } from '@bopsie/client/api/endpoints/players';
import { getSpotifyStatus } from '@bopsie/client/api/endpoints/spotify';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useConnection } from '@/connection/ConnectionProvider';

/**
 * How often the admin views read again while shown. Events keep the players current; what
 * only the admin commands tell, such as health and diagnostics, has no events.
 */
export const POLL_MS = 2_000;

export function usePlayers() {
  const { account, connection } = useConnection();
  return useQuery({
    queryKey: queryKeys.players(account.id),
    queryFn: () => listPlayers(connection),
  });
}

export function useDevices() {
  const { account, connection } = useConnection();
  return useQuery({
    queryKey: queryKeys.admin(account.id, 'devices'),
    queryFn: () => adminDevices(connection),
    refetchInterval: POLL_MS,
  });
}

export function useGroups() {
  const { account, connection } = useConnection();
  return useQuery({
    queryKey: queryKeys.admin(account.id, 'groups'),
    queryFn: () => adminGroups(connection),
    refetchInterval: POLL_MS,
  });
}

export function useServer() {
  const { account, connection } = useConnection();
  return useQuery({
    queryKey: queryKeys.admin(account.id, 'server'),
    queryFn: () => adminServerInfo(connection),
    refetchInterval: POLL_MS,
  });
}

export function useSpotify() {
  const { account, connection } = useConnection();
  return useQuery({
    queryKey: queryKeys.admin(account.id, 'spotify'),
    queryFn: () => getSpotifyStatus(connection),
    refetchInterval: POLL_MS,
  });
}
