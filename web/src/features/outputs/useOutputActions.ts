import {
  joinGroup,
  leaveGroup,
  setGroupVolume,
  setMuted,
  setVolume,
  transferQueue,
} from '@bopsie/client/api/endpoints/players';
import type { Player, PlayerQueue } from '@bopsie/client/api/schemas/player';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import type { Member } from '@bopsie/client/outputs/outputs';
import { MEMBERSHIP_PENDING_MS, type PlayerChanges } from '@bopsie/client/outputs/pending';
import {
  predictGroupVolume,
  predictJoin,
  predictLeave,
  predictMute,
  predictVolume,
} from '@bopsie/client/outputs/predict';
import { useQueryClient } from '@tanstack/react-query';
import { useMemo } from 'react';
import { useConnection } from '@/connection/ConnectionProvider';

/**
 * What the players sheet can do to the group the web controls and the players around it, as the
 * app's output sheet does. Every change shows at once; one the server refuses is withdrawn and
 * handed to `report`.
 */
export function useOutputActions({
  players,
  activeId,
  choose,
  predict,
  forget,
  report,
}: {
  players: Player[] | undefined;
  activeId: string | null;
  /** Makes a player the one the web controls. */
  choose: (playerId: string) => void;
  predict: (changes: PlayerChanges, waitMs?: number) => void;
  forget: (playerIds: string[]) => void;
  report: (error: unknown) => void;
}) {
  const { connection, account } = useConnection();
  const queryClient = useQueryClient();

  return useMemo(() => {
    /** Sends a command whose predicted changes are `changes`. */
    const send = (changes: PlayerChanges, work: Promise<void>, waitMs?: number) => {
      predict(changes, waitMs);
      work.catch((error) => {
        forget(Object.keys(changes));
        report(error);
      });
    };

    /**
     * Moves a queue and the web's control with it. The target shows the queue at once; the
     * server's own copy replaces it once the move lands.
     */
    const moveQueue = (sourceId: string, targetId: string) => {
      const sourceKey = queryKeys.queue(account.id, sourceId);
      const targetKey = queryKeys.queue(account.id, targetId);
      const queue = queryClient.getQueryData<PlayerQueue | null>(sourceKey);
      if (queue) {
        queryClient.setQueryData<PlayerQueue>(targetKey, { ...queue, queue_id: targetId });
        queryClient.setQueryData<PlayerQueue>(sourceKey, {
          ...queue,
          state: 'idle',
          items: 0,
          current_index: null,
          current_item: null,
          next_item: null,
        });
      }
      choose(targetId);
      transferQueue(connection, sourceId, targetId)
        .catch((error) => {
          choose(sourceId);
          report(error);
        })
        .finally(() => {
          void queryClient.invalidateQueries({ queryKey: sourceKey });
          void queryClient.invalidateQueries({ queryKey: targetKey });
        });
    };

    return {
      /** Adds a lone player to the active group. */
      join: (playerId: string) => {
        if (!activeId || !players) return;
        send(
          predictJoin(players, playerId, activeId),
          joinGroup(connection, playerId, activeId),
          MEMBERSHIP_PENDING_MS,
        );
      },

      /** Takes a member out of the active group. Control stays with the music. */
      remove: (member: Member, members: Member[]) => {
        if (!activeId || !players) return;
        // The server hands a departing leader's group to another member.
        const next = member.leads ? members.find((m) => m.id !== member.id) : undefined;
        if (next) choose(next.id);
        send(
          predictLeave(players, member.id, activeId, next?.id),
          leaveGroup(connection, member.id),
          MEMBERSHIP_PENDING_MS,
        );
      },

      /**
       * Moves the active group's queue to another player or group, which becomes the active
       * one. Returns an undo that moves it back.
       */
      move: (targetId: string): (() => void) | null => {
        if (!activeId) return null;
        const sourceId = activeId;
        moveQueue(sourceId, targetId);
        return () => moveQueue(targetId, sourceId);
      },

      /** Makes another player or group the active one. Nothing stops or moves. */
      control: (playerId: string) => choose(playerId),

      setVolume: (playerId: string, level: number) => {
        if (!activeId || !players) return;
        send(
          predictVolume(players, activeId, playerId, level),
          setVolume(connection, playerId, level),
        );
      },

      setMuted: (playerId: string, muted: boolean) =>
        send(predictMute(playerId, muted), setMuted(connection, playerId, muted)),

      setGroupVolume: (level: number) => {
        if (!activeId || !players) return;
        send(
          predictGroupVolume(players, activeId, level),
          setGroupVolume(connection, activeId, level),
        );
      },
    };
  }, [connection, account.id, queryClient, players, activeId, choose, predict, forget, report]);
}
