import { imageUrl } from '@bopsie/client/api/images';
import type { ActiveGroup } from '@bopsie/client/outputs/outputs';
import type { RemoteNowPlaying } from '@/player/Player';
import { advancing } from '@/player/progress';
import { describeItem, type HeardQueue } from './nowPlaying';

/**
 * What the phone's media session shows for the group the app controls, or null when it has
 * nothing to: no group, nothing loaded, or a group the phone plays in, which the phone's own
 * stream already shows. The artist's line names the group, so it says where it plays.
 */
export function remoteNowPlaying(
  queue: HeardQueue | null | undefined,
  active: ActiveGroup | null | undefined,
  phoneId: string | null,
  serverUrl: string,
): RemoteNowPlaying | null {
  const item = queue?.current_item;
  if (!queue || !item || !active) return null;
  if (active.members.some((member) => member.id === phoneId)) return null;
  const described = describeItem(item);
  const media = item.media_item;
  const artists = media ? media.artists.map((artist) => artist.name).join(', ') : '';
  const by = queue.source ? queue.source.subtitle : artists;
  return {
    title: described.title,
    artist: [by, active.name].filter(Boolean).join(' · '),
    album: media?.album?.name ?? null,
    artworkUrl: imageUrl(serverUrl, described.artwork, 512),
    durationMs: item.duration ? Math.round(item.duration * 1000) : null,
    positionMs: Math.round(queue.elapsed_time * 1000),
    positionAt: Math.round(queue.elapsed_time_last_updated * 1000),
    // Still while it buffers, so that the lock screen's position does not run ahead of it.
    playing: advancing(queue),
    volume: active.volume === null ? null : Math.round(active.volume),
  };
}

/** How far the position may drift from where the session puts it before it is told again. */
const DRIFT_MS = 1500;

/**
 * Whether the session needs `next`, having been shown `last`: anything but the position has
 * changed, or the position has moved from where the session, running on from `last`, puts it.
 */
export function changed(last: RemoteNowPlaying | null, next: RemoteNowPlaying | null): boolean {
  if (!last || !next) return last !== next;
  const { positionMs: _, positionAt: __, ...rest } = next;
  const { positionMs: ___, positionAt: ____, ...lastRest } = last;
  if (JSON.stringify(rest) !== JSON.stringify(lastRest)) return true;
  const expected = last.positionMs + (last.playing ? next.positionAt - last.positionAt : 0);
  return Math.abs(expected - next.positionMs) > DRIFT_MS;
}
