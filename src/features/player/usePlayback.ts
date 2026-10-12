import { useQuery } from '@tanstack/react-query';
import { useCallback, useMemo } from 'react';
import { Alert } from 'react-native';
import {
  getQueue,
  playMedia,
  type QueueCommand,
  queueCommand,
  seek,
} from '@/api/endpoints/players';
import { useConnection } from '@/connection/ConnectionProvider';
import { queryKeys } from '@/connection/queryKeys';
import { describeError } from '@/lib/errors';

/** The phone's own queue: what it is playing and where. */
export function useQueue() {
  const { connection, account, queueId } = useConnection();
  return useQuery({
    queryKey: queryKeys.queue(account.id, queueId ?? 'none'),
    queryFn: () => (queueId ? getQueue(connection, queueId) : null),
    enabled: queueId !== null,
    // Events keep the cached queue current.
    staleTime: Number.POSITIVE_INFINITY,
  });
}

function report(error: unknown) {
  Alert.alert('Could not play', describeError(error));
}

/** Plays media on the phone and controls what it plays, through the Music Assistant API. */
export function usePlaybackActions() {
  const { connection, queueId, player } = useConnection();

  const run = useCallback(
    (work: (id: string) => Promise<void>) => {
      if (!queueId || !player.supported) {
        Alert.alert('Not yet', 'Playing on this device is Android-only for now.');
        return;
      }
      work(queueId).catch(report);
    },
    [queueId, player],
  );

  return useMemo(
    () => ({
      /** Replaces the queue with `uri` (an album, playlist, artist or track) and plays it. */
      play: (uri: string, startItem?: string) =>
        run((id) => playMedia(connection, id, uri, startItem ? { startItem } : {})),
      command: (command: QueueCommand) => run((id) => queueCommand(connection, id, command)),
      seek: (seconds: number) => run((id) => seek(connection, id, seconds)),
    }),
    [connection, run],
  );
}
