import { type ConnectionStatus, MaConnection } from '@bopsie/client/api/client';
import type { Account } from '@bopsie/client/auth/types';
import { applyEvent } from '@bopsie/client/connection/eventBridge';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQueryClient } from '@tanstack/react-query';
import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from 'react';
import { useAccountActions, useActiveAccount, useAuthContext } from '@/auth/AuthProvider';

interface ConnectionContextValue {
  account: Account;
  connection: MaConnection;
  /** For the requests that are not commands, such as uploading artwork. */
  token: string;
  status: ConnectionStatus;
}

const ConnectionContext = createContext<ConnectionContextValue | null>(null);

/**
 * The active account's API connection. It waits for the account's token, then stays open,
 * feeding server events into the query cache. A token the server refuses marks the account
 * expired.
 */
export function ConnectionProvider({ children }: { children: ReactNode }) {
  const account = useActiveAccount();
  const { repository } = useAuthContext();
  const { markExpired } = useAccountActions();
  const queryClient = useQueryClient();
  const [opened, setOpened] = useState<{ connection: MaConnection; token: string } | null>(null);
  const [status, setStatus] = useState<ConnectionStatus>('connecting');

  useEffect(() => {
    if (!account) return;
    let cancelled = false;
    let socket: MaConnection | null = null;
    const unsubscribes: Array<() => void> = [];

    void repository.loadToken(account.id).then((token) => {
      if (cancelled) return;
      if (!token) {
        markExpired(account.id);
        return;
      }
      const connection = new MaConnection({ serverUrl: account.serverUrl, token });
      socket = connection;
      unsubscribes.push(
        connection.onEvent((event) => applyEvent(queryClient, account.id, event)),
        connection.onStatus((next) => {
          setStatus(next);
          if (next === 'unauthorized') markExpired(account.id);
          // Events were missed while disconnected.
          if (next === 'ready') {
            void queryClient.invalidateQueries({ queryKey: queryKeys.queues(account.id) });
            void queryClient.invalidateQueries({ queryKey: queryKeys.players(account.id) });
          }
        }),
      );
      connection.connect();
      setOpened({ connection, token });
    });

    return () => {
      cancelled = true;
      for (const unsubscribe of unsubscribes) unsubscribe();
      socket?.close();
      setOpened(null);
    };
  }, [account, repository, markExpired, queryClient]);

  const value = useMemo(
    () => (account && opened ? { account, ...opened, status } : null),
    [account, opened, status],
  );
  if (!value) return null;
  return <ConnectionContext.Provider value={value}>{children}</ConnectionContext.Provider>;
}

export function useConnection(): ConnectionContextValue {
  const context = useContext(ConnectionContext);
  if (!context) throw new Error('useConnection must be used inside ConnectionProvider');
  return context;
}
