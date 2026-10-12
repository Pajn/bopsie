import * as z from 'zod/mini';

/** The first frame on every `/ws` connection (`ServerInfoMessage`). */
export const serverInfo = z.looseObject({
  server_id: z.string(),
  server_version: z.string(),
  schema_version: z.number(),
  min_supported_schema_version: z.optional(z.number()),
  name: z.optional(z.string()),
  onboard_done: z.optional(z.boolean()),
});
export type ServerInfo = z.infer<typeof serverInfo>;

/** Schema 28 is Music Assistant 2.7, the first release with users and tokens. */
export const MIN_SCHEMA_VERSION = 28;
