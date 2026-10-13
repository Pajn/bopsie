import { useCallback, useMemo } from 'react';
import { Alert } from 'react-native';
import {
  joinGroup,
  leaveGroup,
  setGroupVolume,
  setVolume,
  transferQueue,
} from '@/api/endpoints/players';
import { useConnection } from '@/connection/ConnectionProvider';
import { describeError } from '@/lib/errors';
import { useActiveGroup } from './ActiveGroupProvider';
import { describeOutputs, type Member, type Outputs } from './outputs';

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

/** What the output sheet can do to the active group and the players around it. */
export function useOutputActions() {
  const { connection } = useConnection();
  const { activeId, choose } = useActiveGroup();

  const run = useCallback((work: Promise<void>) => work.catch(report), []);

  return useMemo(
    () => ({
      /** Adds a lone player to the active group. */
      join: (playerId: string) => {
        if (activeId) void run(joinGroup(connection, playerId, activeId));
      },

      /** Takes a member out of the active group. Control stays with the music. */
      remove: (member: Member, members: Member[]) => {
        // The server hands a departing leader's group to another member.
        const next = member.leads ? members.find((m) => m.id !== member.id) : undefined;
        if (next) choose(next.id);
        void run(leaveGroup(connection, member.id));
      },

      /**
       * Moves the active group's queue to another player or group, which
       * becomes the active one. Resolves to an undo that moves it back, or
       * null when the move failed.
       */
      move: async (targetId: string): Promise<(() => void) | null> => {
        if (!activeId) return null;
        const sourceId = activeId;
        try {
          await transferQueue(connection, sourceId, targetId);
        } catch (error) {
          report(error);
          return null;
        }
        choose(targetId);
        return () => {
          choose(sourceId);
          void run(transferQueue(connection, targetId, sourceId));
        };
      },

      /** Makes another player or group the active one. Nothing stops or moves. */
      control: (playerId: string) => choose(playerId),

      setVolume: (playerId: string, level: number) =>
        void run(setVolume(connection, playerId, level)),

      setGroupVolume: (level: number) => {
        if (activeId) void run(setGroupVolume(connection, activeId, level));
      },
    }),
    [connection, activeId, choose, run],
  );
}
