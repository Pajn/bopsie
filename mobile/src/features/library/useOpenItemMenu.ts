import type { Album, Artist, Playlist, Track } from '@bopsie/client/api/schemas/media';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQueryClient } from '@tanstack/react-query';
import { useRouter } from 'expo-router';
import { useCallback } from 'react';
import { useConnection } from '@/connection/ConnectionProvider';

export type MenuKind = 'track' | 'album' | 'artist' | 'playlist';
export type MenuItem = Track | Album | Artist | Playlist;

/**
 * Opens the menu for an item. The item is handed to the menu through the query
 * cache, where its page would look for it too, so the sheet opens at its full
 * size rather than growing once the item loads.
 */
export function useOpenItemMenu() {
  const router = useRouter();
  const queryClient = useQueryClient();
  const { account } = useConnection();
  return useCallback(
    (kind: MenuKind, item: MenuItem) => {
      const key = queryKeys.item(account.id, kind, item.item_id, item.provider);
      if (queryClient.getQueryData(key) === undefined) queryClient.setQueryData(key, item);
      router.push({
        pathname: '/item-menu',
        params: { kind, id: item.item_id, provider: item.provider },
      });
    },
    [account.id, queryClient, router],
  );
}
