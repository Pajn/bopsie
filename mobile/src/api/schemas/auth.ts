import * as z from 'zod/mini';
import { stringOr } from './common';

export const user = z.looseObject({
  user_id: z.string(),
  username: z.string(),
  display_name: z.nullish(z.string()),
  role: stringOr('user'),
});
export type User = z.infer<typeof user>;

/**
 * The result of `auth/login`. A wrong password is a successful command whose
 * result says `success: false`, not an error frame.
 */
export const loginResult = z.union([
  z.looseObject({ success: z.literal(true), access_token: z.string(), user }),
  z.looseObject({ success: z.literal(false), error: z.optional(z.string()) }),
]);

/** The result of `auth/token/create`: the token itself. */
export const createdToken = z.string();
