import type { Track } from '@bopsie/client/api/schemas/media';
import { Link } from 'expo-router';
import { Fragment } from 'react';
import { Text } from '@/ui/primitives';
import { itemHref } from './links';

/**
 * A track's artists and album, in a line, each a link to its page: "Artist, Artist · Album".
 * Nothing when it names neither. The links take the line's colour.
 */
export function Credits({
  track,
  variant = 'body',
}: {
  track: Pick<Track, 'artists' | 'album'>;
  variant?: 'body' | 'caption';
}) {
  const { artists, album } = track;
  if (artists.length === 0 && !album) return null;
  return (
    <Text variant={variant} muted numberOfLines={1}>
      {artists.map((artist, i) => (
        <Fragment key={`${artist.provider}-${artist.item_id}`}>
          {i > 0 ? ', ' : null}
          <Link href={itemHref(artist) as never}>{artist.name}</Link>
        </Fragment>
      ))}
      {artists.length > 0 && album ? ' · ' : null}
      {album ? <Link href={itemHref(album) as never}>{album.name}</Link> : null}
    </Text>
  );
}
