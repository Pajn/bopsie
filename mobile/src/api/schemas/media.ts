import * as z from 'zod/mini';
import { arrayOr, booleanOr, numberOr, stringOr } from './common';

/** `MediaItemImage`. The server adds `proxy_id` from 2.10; 2.7–2.9 send provider and path only. */
export const image = z.looseObject({
  type: stringOr('thumb'),
  path: z.string(),
  provider: z.string(),
  remotely_accessible: booleanOr(false),
  proxy_id: z.nullish(z.string()),
});
export type MediaImage = z.infer<typeof image>;

const metadata = z.pipe(
  z.nullish(z.looseObject({ images: arrayOr(image) })),
  z.transform((value) => value ?? { images: [] }),
);

/** `ItemMapping`: a light reference to a media item, as used inside other items. */
export const itemMapping = z.looseObject({
  item_id: z.string(),
  provider: z.string(),
  name: z.string(),
  uri: z.string(),
  media_type: z.string(),
  image: z.nullish(image),
});
export type ItemMapping = z.infer<typeof itemMapping>;

const mediaItemFields = {
  item_id: z.string(),
  provider: z.string(),
  name: z.string(),
  uri: z.string(),
  media_type: z.string(),
  sort_name: z.nullish(z.string()),
  favorite: booleanOr(false),
  metadata,
};

export const artist = z.looseObject({ ...mediaItemFields });
export type Artist = z.infer<typeof artist>;

export const album = z.looseObject({
  ...mediaItemFields,
  artists: arrayOr(itemMapping),
  year: z.nullish(z.number()),
  album_type: stringOr('unknown'),
});
export type Album = z.infer<typeof album>;

export const track = z.looseObject({
  ...mediaItemFields,
  duration: numberOr(0),
  artists: arrayOr(itemMapping),
  album: z.nullish(itemMapping),
  track_number: z.nullish(z.number()),
  disc_number: z.nullish(z.number()),
});
export type Track = z.infer<typeof track>;

export const playlist = z.looseObject({
  ...mediaItemFields,
  owner: z.nullish(z.string()),
  is_editable: booleanOr(false),
});
export type Playlist = z.infer<typeof playlist>;

export const searchResults = z.looseObject({
  artists: arrayOr(artist),
  albums: arrayOr(album),
  tracks: arrayOr(track),
  playlists: arrayOr(playlist),
});
export type SearchResults = z.infer<typeof searchResults>;

/** Anything with images: a full media item or an item mapping. */
export type WithImages =
  | { metadata: { images: MediaImage[] } }
  | { image?: MediaImage | null | undefined };
