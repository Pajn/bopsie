import type { Player } from '@bopsie/client/api/schemas/player';
import {
  addPending,
  applyPending,
  dropPending,
  joiningOrLeaving,
  type Pending,
  type PlayerChanges,
  settlePending,
} from '@bopsie/client/outputs/pending';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { usePlayers } from '@/data/admin';

/**
 * Every player, kept current by events, as it will be once the changes asked of it land: a
 * change shows at once, until the server confirms or overrules it, or it has waited too long.
 */
export function usePredictedPlayers() {
  const server = usePlayers().data;
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

  const predict = useCallback((changes: PlayerChanges, waitMs?: number) => {
    const players = latest.current;
    if (players) setPending((current) => addPending(players, current, changes, Date.now(), waitMs));
  }, []);
  const forget = useCallback(
    (playerIds: string[]) => setPending((current) => dropPending(current, playerIds)),
    [],
  );

  const players: Player[] | undefined = useMemo(
    () => server && applyPending(server, pending),
    [server, pending],
  );
  /** Players whose move into or out of a group the server has not confirmed. */
  const unconfirmed = useMemo(() => joiningOrLeaving(pending), [pending]);
  return { players, unconfirmed, predict, forget };
}
