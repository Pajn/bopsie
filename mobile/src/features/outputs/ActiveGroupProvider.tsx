import { useQuery } from '@tanstack/react-query';
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { listPlayers } from '@/api/endpoints/players';
import type { Player } from '@/api/schemas/player';
import { sqliteKeyValueStore } from '@/auth/platformStores';
import { useConnection } from '@/connection/ConnectionProvider';
import { queryKeys } from '@/connection/queryKeys';
import { activeLeaderId } from './outputs';
import {
  addPending,
  applyPending,
  dropPending,
  joiningOrLeaving,
  type Pending,
  type PlayerChanges,
  settlePending,
} from './pending';

interface ActiveGroupValue {
  /** Every player on the server, kept current by events, with changes not yet confirmed. */
  players: Player[] | undefined;
  /** The leader of the group the app controls, which is also its queue id. */
  activeId: string | null;
  /** Makes the group `playerId` belongs to the one the app controls. */
  choose: (playerId: string) => void;
  /** Players whose move into or out of a group the server has not confirmed. */
  unconfirmed: ReadonlySet<string>;
  /** Shows changes the app has asked for until the server confirms them, or `waitMs` passes. */
  predict: (changes: PlayerChanges, waitMs?: number) => void;
  /** Withdraws predictions for these players, as when the server refused a command. */
  forget: (playerIds: string[]) => void;
}

const ActiveGroupContext = createContext<ActiveGroupValue | null>(null);

const storageKey = (accountId: string) => `players.active.${accountId}`;

/**
 * Which player the app controls. The choice is remembered per account; until
 * one is made, it is the phone. Changes the app makes to players show at
 * once, ahead of the server's confirmation.
 */
export function ActiveGroupProvider({ children }: { children: ReactNode }) {
  const { connection, account, queueId: phoneId } = useConnection();
  const server = useQuery({
    queryKey: queryKeys.players(account.id),
    queryFn: () => listPlayers(connection),
    // Events keep the cached list current.
    staleTime: Number.POSITIVE_INFINITY,
  }).data;
  // The connection, and this provider with it, is created afresh for each account.
  const [chosenId, setChosenId] = useState(() => sqliteKeyValueStore.get(storageKey(account.id)));
  const [pending, setPending] = useState<Pending[]>([]);
  const latest = useRef(server);
  latest.current = server;

  // Each server update may confirm, or overrule, a prediction.
  useEffect(() => {
    if (server) setPending((current) => settlePending(server, current, Date.now()));
  }, [server]);

  // Predictions the server never answers expire.
  useEffect(() => {
    if (pending.length === 0) return;
    const due = Math.min(...pending.map((p) => p.until)) - Date.now();
    const timer = setTimeout(
      () => setPending((current) => settlePending(latest.current ?? [], current, Date.now())),
      Math.max(0, due),
    );
    return () => clearTimeout(timer);
  }, [pending]);

  const choose = useCallback(
    (playerId: string) => {
      sqliteKeyValueStore.set(storageKey(account.id), playerId);
      setChosenId(playerId);
    },
    [account.id],
  );
  const predict = useCallback((changes: PlayerChanges, waitMs?: number) => {
    const players = latest.current;
    if (players) setPending((current) => addPending(players, current, changes, Date.now(), waitMs));
  }, []);
  const forget = useCallback(
    (playerIds: string[]) => setPending((current) => dropPending(current, playerIds)),
    [],
  );

  const players = useMemo(() => server && applyPending(server, pending), [server, pending]);
  const unconfirmed = useMemo(() => joiningOrLeaving(pending), [pending]);
  const activeId = activeLeaderId(players, chosenId, phoneId);
  const value = useMemo(
    () => ({ players, activeId, choose, unconfirmed, predict, forget }),
    [players, activeId, choose, unconfirmed, predict, forget],
  );
  return <ActiveGroupContext.Provider value={value}>{children}</ActiveGroupContext.Provider>;
}

export function useActiveGroup(): ActiveGroupValue {
  const context = useContext(ActiveGroupContext);
  if (!context) throw new Error('useActiveGroup must be used inside ActiveGroupProvider');
  return context;
}
