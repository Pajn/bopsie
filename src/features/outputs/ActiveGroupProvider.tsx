import { useQuery } from '@tanstack/react-query';
import { createContext, type ReactNode, useCallback, useContext, useMemo, useState } from 'react';
import { listPlayers } from '@/api/endpoints/players';
import type { Player } from '@/api/schemas/player';
import { sqliteKeyValueStore } from '@/auth/platformStores';
import { useConnection } from '@/connection/ConnectionProvider';
import { queryKeys } from '@/connection/queryKeys';
import { activeLeaderId } from './outputs';

interface ActiveGroupValue {
  /** Every player on the server, kept current by events. */
  players: Player[] | undefined;
  /** The leader of the group the app controls, which is also its queue id. */
  activeId: string | null;
  /** Makes the group `playerId` belongs to the one the app controls. */
  choose: (playerId: string) => void;
}

const ActiveGroupContext = createContext<ActiveGroupValue | null>(null);

const storageKey = (accountId: string) => `players.active.${accountId}`;

/**
 * Which player the app controls. The choice is remembered per account; until
 * one is made, it is the phone.
 */
export function ActiveGroupProvider({ children }: { children: ReactNode }) {
  const { connection, account, queueId: phoneId } = useConnection();
  const players = useQuery({
    queryKey: queryKeys.players(account.id),
    queryFn: () => listPlayers(connection),
    // Events keep the cached list current.
    staleTime: Number.POSITIVE_INFINITY,
  }).data;
  // The connection, and this provider with it, is created afresh for each account.
  const [chosenId, setChosenId] = useState(() => sqliteKeyValueStore.get(storageKey(account.id)));

  const choose = useCallback(
    (playerId: string) => {
      sqliteKeyValueStore.set(storageKey(account.id), playerId);
      setChosenId(playerId);
    },
    [account.id],
  );

  const activeId = activeLeaderId(players, chosenId, phoneId);
  const value = useMemo(() => ({ players, activeId, choose }), [players, activeId, choose]);
  return <ActiveGroupContext.Provider value={value}>{children}</ActiveGroupContext.Provider>;
}

export function useActiveGroup(): ActiveGroupValue {
  const context = useContext(ActiveGroupContext);
  if (!context) throw new Error('useActiveGroup must be used inside ActiveGroupProvider');
  return context;
}
