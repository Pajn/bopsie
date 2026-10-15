import { useEffect, useEffectEvent, useRef } from 'react';
import { useConnection } from '@/connection/ConnectionProvider';
import { useOutputActions, useOutputs } from '@/features/outputs/useOutputs';
import type { RemoteCommand, RemoteNowPlaying } from '@/player/Player';
import { changed, remoteNowPlaying } from './remoteState';
import { usePlaybackActions, useQueue } from './usePlayback';

/**
 * Shows the group the app controls in the phone's media session while the phone plays nothing
 * itself, and does what the session asks of it: its notification, lock screen controls and
 * volume keys then work the group the app does. Draws nothing.
 */
export function RemoteSession() {
  const { player, queueId: phoneId, account } = useConnection();
  const queue = useQueue().data;
  const active = useOutputs()?.active;
  const playback = usePlaybackActions();
  const outputs = useOutputActions();
  const shown = useRef<RemoteNowPlaying | null>(null);

  const next = remoteNowPlaying(queue, active, phoneId, account.serverUrl);
  useEffect(() => {
    if (!changed(shown.current, next)) return;
    shown.current = next;
    player.showRemote(next);
  });

  // Stops showing the group when the app stops following it, as on signing out.
  useEffect(
    () => () => {
      shown.current = null;
      player.showRemote(null);
    },
    [player],
  );

  const run = useEffectEvent((command: RemoteCommand) => {
    switch (command.command) {
      case 'seek':
        playback.seek(command.positionMs / 1000);
        break;
      case 'volume':
        outputs.setGroupVolume(command.level);
        break;
      default:
        playback.command(command.command);
    }
  });
  useEffect(() => player.onRemoteCommand((command) => run(command)), [player]);

  return null;
}
