import { useQueryClient } from '@tanstack/react-query';
import { useMemo } from 'react';
import { Alert } from 'react-native';
import {
  joinGroup,
  leaveGroup,
  setGroupVolume,
  setVolume,
  transferQueue,
} from '@/api/endpoints/players';
import type { PlayerQueue } from '@/api/schemas/player';
import { useConnection } from '@/connection/ConnectionProvider';
import { queryKeys } from '@/connection/queryKeys';
import { describeError } from '@/lib/errors';
import { useActiveGroup } from './ActiveGroupProvider';
import { describeOutputs, type Member, type Outputs } from './outputs';
import { MEMBERSHIP_PENDING_MS, type PlayerChanges } from './pending';
import { predictGroupVolume, predictJoin, predictLeave, predictVolume } from './predict';

/** The active group and every other player, as the output sheet shows them. */
export function useOutputs(): Outputs | null {
  const { queueId: phoneId } = useConnection();
  const { players, activeId } = useActiveGroup();
  return useMemo(
    () => (players ? describeOutputs(players, activeId, phoneId) : null),
    [players, activeId, phoneId],
  );
}

function report(error: unknown) {
  Alert.alert('Could not change players', describeError(error));
}

/**
 * What the output sheet can do to the active group and the players around it.
 * Every change shows at once; one the server refuses is withdrawn and
 * reported.
 */
export function useOutputActions() {
  const { connection, account } = useConnection();
  const { players, activeId, choose, predict, forget } = useActiveGroup();
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
     * Moves a queue and the app's control with it. The target shows the
     * queue at once; the server's own copy replaces it once the move lands.
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
       * Moves the active group's queue to another player or group, which
       * becomes the active one. Returns an undo that moves it back.
       */
      move: (targetId: string): (() => void) | null => {
        if (!activeId) return null;
        const sourceId = activeId;
        moveQueue(sourceId, targetId);
        return () => moveQueue(targetId, sourceId);
      },

      /** Makes another player or group the active one. Nothing stops or moves. */
      control: (playerId: string) => choose(playerId),

      setVolume: (playerId: string, level: number) =>
        send(predictVolume(playerId, level), setVolume(connection, playerId, level)),

      setGroupVolume: (level: number) => {
        if (!activeId || !players) return;
        send(
          predictGroupVolume(players, activeId, level),
          setGroupVolume(connection, activeId, level),
        );
      },
    };
  }, [connection, account.id, queryClient, players, activeId, choose, predict, forget]);
}
