import {
  getQueue,
  playMedia,
  type QueueCommand,
  queueCommand,
  seek,
  setRepeat,
  setShuffle,
} from '@bopsie/client/api/endpoints/players';
import type { RepeatMode } from '@bopsie/client/api/schemas/player';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useRouter } from 'expo-router';
import { useCallback, useMemo } from 'react';
import { Alert } from 'react-native';
import { useConnection } from '@/connection/ConnectionProvider';
import { useActiveGroup } from '@/features/outputs/ActiveGroupProvider';
import { describeError } from '@/lib/errors';
import { asHeard } from './nowPlaying';

/**
 * The active group's queue, as the listener hears it: what it is playing and
 * where, or what a source such as Spotify Connect plays to it instead.
 */
export function useQueue() {
  const { connection, account } = useConnection();
  const { activeId, players } = useActiveGroup();
  const query = useQuery({
    queryKey: queryKeys.queue(account.id, activeId ?? 'none'),
    queryFn: () => (activeId ? getQueue(connection, activeId) : null),
    enabled: activeId !== null,
    // Events keep the cached queue current.
    staleTime: Number.POSITIVE_INFINITY,
  });
  const player = players?.find((candidate) => candidate.player_id === activeId);
  const data = useMemo(
    () => (query.data ? asHeard(query.data, player) : query.data),
    [query.data, player],
  );
  return { ...query, data };
}

function report(error: unknown) {
  Alert.alert('Could not play', describeError(error));
}

/** Plays media on the active group and controls what it plays, through the Music Assistant API. */
export function usePlaybackActions() {
  const { connection } = useConnection();
  const { activeId } = useActiveGroup();
  const router = useRouter();

  const run = useCallback(
    (work: (id: string) => Promise<void>) => {
      // Only where the phone cannot play itself and no other player is chosen yet.
      if (!activeId) {
        router.push('/outputs');
        return;
      }
      work(activeId).catch(report);
    },
    [activeId, router],
  );

  return useMemo(
    () => ({
      /** Replaces the queue with `uri` (an album, playlist, artist or track) and plays it. */
      play: (uri: string, startItem?: string) =>
        run((id) => playMedia(connection, id, uri, startItem ? { startItem } : {})),
      /** Turns shuffle on, then replaces the queue with `uri`, which the server shuffles. */
      shufflePlay: (uri: string) =>
        run(async (id) => {
          await setShuffle(connection, id, true);
          await playMedia(connection, id, uri);
        }),
      /** Puts `uri` after the current item, leaving the rest of the queue in place. */
      playNext: (uri: string) => run((id) => playMedia(connection, id, uri, { option: 'next' })),
      /** Puts `uri` at the end of the queue. */
      addToQueue: (uri: string) => run((id) => playMedia(connection, id, uri, { option: 'add' })),
      command: (command: QueueCommand) => run((id) => queueCommand(connection, id, command)),
      seek: (seconds: number) => run((id) => seek(connection, id, seconds)),
      shuffle: (enabled: boolean) => run((id) => setShuffle(connection, id, enabled)),
      repeat: (mode: RepeatMode) => run((id) => setRepeat(connection, id, mode)),
    }),
    [connection, run],
  );
}
