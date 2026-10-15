import { useRouter } from 'expo-router';
import { type ItemLink, linkHref } from '@/features/library/links';
import { MenuHeader, MenuRow, MenuSheet } from '@/features/library/menu';
import { itemLinks, nowPlaying } from './nowPlaying';
import { collapsePlayer } from './sheetStore';
import { useQueue } from './usePlayback';

/**
 * What the full player's menu offers for the current track: its album and
 * each of its artists. Choosing one closes the player and opens that page.
 */
export function PlayerMenu() {
  const router = useRouter();
  const queue = useQueue().data;
  const item = queue?.current_item;
  const current = queue ? nowPlaying(queue) : null;
  if (!item || !current) return null;

  const open = (link: ItemLink) => {
    // Closing this sheet and the player, on to the page beneath them.
    router.back();
    collapsePlayer();
    router.push(linkHref(link));
  };

  return (
    <MenuSheet testID="player-menu">
      <MenuHeader item={current.artwork} title={current.title} subtitle={current.subtitle} />
      {itemLinks(item).map((link) => (
        <MenuRow
          key={`${link.kind}-${link.provider}-${link.id}`}
          icon={link.kind === 'album' ? 'disc' : 'user'}
          label={link.kind === 'album' ? 'Go to album' : 'Go to artist'}
          detail={link.name}
          onPress={() => open(link)}
          testID={`player-menu-${link.kind}-${link.id}`}
        />
      ))}
    </MenuSheet>
  );
}
