import { useQueryClient } from '@tanstack/react-query';
import Constants from 'expo-constants';
import { createContext, type ReactNode, useContext, useEffect, useMemo, useState } from 'react';
import { Platform } from 'react-native';
import { type ConnectionStatus, MaConnection } from '@/api/client';
import { socketUrl } from '@/api/url';
import { useAccountActions, useActiveAccount, useAuthContext } from '@/auth/AuthProvider';
import type { Account } from '@/auth/types';
import { createPlayer } from '@/player/createPlayer';
import type { LocalPlayer, PlayerStatus } from '@/player/Player';
import { applyEvent } from './eventBridge';
import { queryKeys } from './queryKeys';

interface ConnectionContextValue {
  account: Account;
  connection: MaConnection;
  status: ConnectionStatus;
  player: LocalPlayer;
  playerStatus: PlayerStatus;
  /** The phone's player id, which is also its queue id. Null where playback is unsupported. */
  queueId: string | null;
}

const ConnectionContext = createContext<ConnectionContextValue | null>(null);

/** Created once: the native service outlives account changes. */
const player = createPlayer();

function deviceName(): string {
  const model =
    Platform.OS === 'android' ? (Platform.constants as { Model?: string }).Model : undefined;
  return model ? `Bopsie on ${model}` : 'Bopsie';
}

/**
 * The active account's API connection and the phone's player. The connection
 * waits for the account's token, then stays open, feeding server events into
 * the query cache. A token the server refuses marks the account expired.
 */
export function ConnectionProvider({ children }: { children: ReactNode }) {
  const account = useActiveAccount();
  const { repository } = useAuthContext();
  const { markExpired } = useAccountActions();
  const queryClient = useQueryClient();
  const [state, setState] = useState<{ connection: MaConnection; token: string } | null>(null);
  const [status, setStatus] = useState<ConnectionStatus>('connecting');
  const [playerStatus, setPlayerStatus] = useState<PlayerStatus>('closed');
  const queueId = useMemo(() => player.clientId(), []);

  useEffect(() => player.subscribe((next) => setPlayerStatus(next)), []);

  useEffect(() => {
    if (!account) return;
    let cancelled = false;
    let connection: MaConnection | null = null;
    const unsubscribes: Array<() => void> = [];

    void repository.loadToken(account.id).then((token) => {
      if (cancelled) return;
      if (!token) {
        markExpired(account.id);
        return;
      }
      connection = new MaConnection({ serverUrl: account.serverUrl, token });
      unsubscribes.push(
        connection.onEvent((event) => applyEvent(queryClient, account.id, queueId, event)),
        connection.onStatus((next) => {
          setStatus(next);
          if (next === 'unauthorized') markExpired(account.id);
          // Events were missed while disconnected.
          if (next === 'ready' && queueId) {
            void queryClient.invalidateQueries({ queryKey: queryKeys.queue(account.id, queueId) });
          }
        }),
      );
      connection.connect();
      setState({ connection, token });

      if (player.supported) {
        void player.connect({
          serverUrl: account.serverUrl,
          url: socketUrl(account.serverUrl, '/sendspin'),
          token,
          name: deviceName(),
          softwareVersion: Constants.expoConfig?.version ?? '0.0.0',
        });
      }
    });

    return () => {
      cancelled = true;
      for (const unsubscribe of unsubscribes) unsubscribe();
      connection?.close();
      setState(null);
      void player.disconnect();
    };
  }, [account, repository, markExpired, queryClient, queueId]);

  useEffect(() => {
    if (account && playerStatus === 'unauthorized') markExpired(account.id);
  }, [account, playerStatus, markExpired]);

  const value = useMemo(
    () =>
      account && state
        ? { account, connection: state.connection, status, player, playerStatus, queueId }
        : null,
    [account, state, status, playerStatus, queueId],
  );
  if (!value) return null;
  return <ConnectionContext.Provider value={value}>{children}</ConnectionContext.Provider>;
}

export function useConnection(): ConnectionContextValue {
  const context = useContext(ConnectionContext);
  if (!context) throw new Error('useConnection must be used inside ConnectionProvider');
  return context;
}
