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

/** `MediaItemChapter`: a chapter of an audiobook, its times in seconds into the book. */
export const chapter = z.looseObject({
  position: z.number(),
  name: stringOr(''),
  start: z.number(),
  end: z.nullish(z.number()),
});
export type Chapter = z.infer<typeof chapter>;

const metadata = z.pipe(
  z.nullish(
    z.looseObject({
      images: arrayOr(image),
      description: z.nullish(z.string()),
      /** An audiobook's, as a book in a queue carries them too. */
      chapters: arrayOr(chapter),
    }),
  ),
  z.transform((value) => value ?? { images: [], description: null, chapters: [] }),
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

/** `ProviderMapping`: one provider that holds an item; the first listed plays first. */
export const providerMapping = z.looseObject({
  item_id: z.string(),
  provider_domain: z.string(),
  provider_instance: z.string(),
  available: booleanOr(true),
});
export type ProviderMapping = z.infer<typeof providerMapping>;

const mediaItemFields = {
  item_id: z.string(),
  provider: z.string(),
  name: z.string(),
  uri: z.string(),
  media_type: z.string(),
  sort_name: z.nullish(z.string()),
  favorite: booleanOr(false),
  metadata,
  provider_mappings: arrayOr(providerMapping),
  /** What identifies it beyond its name, as `[kind, value]`: ISRCs, MusicBrainz ids. */
  external_ids: arrayOr(z.tuple([z.string(), z.string()])),
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

/** `Audiobook`, from a source such as Audiobookshelf; the library stores none. */
export const audiobook = z.looseObject({
  ...mediaItemFields,
  /** As artists of the source that serves the book, each leading to their other books. */
  authors: arrayOr(itemMapping),
  narrators: arrayOr(z.string()),
  publisher: z.nullish(z.string()),
  /** Seconds. */
  duration: numberOr(0),
  fully_played: z.nullish(z.boolean()),
  /** Where listening left off; null when it was never started. */
  resume_position_ms: z.nullish(z.number()),
});
export type Audiobook = z.infer<typeof audiobook>;

export const searchResults = z.looseObject({
  artists: arrayOr(artist),
  albums: arrayOr(album),
  tracks: arrayOr(track),
  playlists: arrayOr(playlist),
  audiobooks: arrayOr(audiobook),
});
export type SearchResults = z.infer<typeof searchResults>;

/** Anything with images: a full media item or an item mapping. */
export type WithImages =
  | { metadata: { images: MediaImage[] } }
  | { image?: MediaImage | null | undefined };

/** `ProviderInstance`: a source of music, scanned into the library or searched as it is. */
export const musicProvider = z.looseObject({
  instance_id: z.string(),
  domain: z.string(),
  name: z.string(),
  /** A catalog, such as Spotify, searched and browsed as it is rather than scanned in. */
  is_streaming_provider: booleanOr(false),
});
export type MusicProvider = z.infer<typeof musicProvider>;
