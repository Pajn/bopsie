import { getAlbum, getArtist, getPlaylist, getTrack } from '@bopsie/client/api/endpoints/library';
import type { Album, Playlist, Track } from '@bopsie/client/api/schemas/media';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useRouter } from 'expo-router';
import { useConnection } from '@/connection/ConnectionProvider';
import { usePlaybackActions } from '@/features/player/usePlayback';
import { type ItemLink, linkHref, mediaLinks } from './links';
import { MenuHeader, MenuRow, MenuSheet } from './menu';
import { artistNames } from './rows';
import type { MenuItem, MenuKind } from './useOpenItemMenu';

const fetchers = {
  track: getTrack,
  album: getAlbum,
  artist: getArtist,
  playlist: getPlaylist,
} as const;

/**
 * What can be done with a track, album, artist or playlist: play it after the
 * current item or at the end of the queue, or open its album or artists.
 */
export function ItemMenu({ kind, id, provider }: { kind: MenuKind; id: string; provider: string }) {
  const router = useRouter();
  const { connection, account } = useConnection();
  const playback = usePlaybackActions();
  const item = useQuery<MenuItem>({
    queryKey: queryKeys.item(account.id, kind, id, provider),
    queryFn: () => fetchers[kind](connection, { itemId: id, provider }),
  }).data;
  if (!item) return null;

  const close = (then: () => void) => {
    router.back();
    then();
  };
  const open = (link: ItemLink) => close(() => router.push(linkHref(link)));
  // An album leads on to its artists, a track to its album too; the others lead nowhere.
  const links = kind === 'track' || kind === 'album' ? mediaLinks(item as Track | Album) : [];

  return (
    <MenuSheet testID="item-menu">
      <MenuHeader
        item={item}
        title={item.name}
        subtitle={subtitle(kind, item)}
        round={kind === 'artist'}
        placeholder={kind === 'artist' ? 'user' : kind === 'playlist' ? 'list' : 'disc'}
      />
      <MenuRow
        icon="corner-down-right"
        label="Play next"
        onPress={() => close(() => playback.playNext(item.uri))}
        testID="item-menu-next"
      />
      <MenuRow
        icon="list"
        label="Add to queue"
        onPress={() => close(() => playback.addToQueue(item.uri))}
        testID="item-menu-add"
      />
      {links.map((link) => (
        <MenuRow
          key={`${link.kind}-${link.provider}-${link.id}`}
          icon={link.kind === 'album' ? 'disc' : 'user'}
          label={link.kind === 'album' ? 'Go to album' : 'Go to artist'}
          detail={link.name}
          onPress={() => open(link)}
          testID={`item-menu-${link.kind}-${link.id}`}
        />
      ))}
    </MenuSheet>
  );
}

function subtitle(kind: MenuKind, item: MenuItem): string {
  switch (kind) {
    case 'track':
      return [artistNames(item as Track), (item as Track).album?.name].filter(Boolean).join(' · ');
    case 'album':
      return [artistNames(item as Album), (item as Album).year].filter(Boolean).join(' · ');
    case 'playlist':
      return (item as Playlist).owner ?? '';
    case 'artist':
      return '';
  }
}
