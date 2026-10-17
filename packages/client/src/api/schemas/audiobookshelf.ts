import * as z from 'zod/mini';

/** Which Audiobookshelf server the server takes audiobooks from, and who it is signed in as. */
export const audiobookshelfStatus = z.looseObject({
  url: z.nullable(z.string()),
  /** Who it is signed in as, or was last. */
  user: z.nullable(z.string()),
  signed_in: z.boolean(),
});
export type AudiobookshelfStatus = z.infer<typeof audiobookshelfStatus>;
