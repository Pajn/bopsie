import * as z from 'zod/mini';
import { booleanOr, stringOr } from './common';

/** Music Assistant's `User`, as user management reads it. */
export const managedUser = z.looseObject({
  user_id: z.string(),
  username: z.string(),
  display_name: z.nullish(z.string()),
  role: stringOr('user'),
  enabled: booleanOr(true),
  created_at: z.nullish(z.string()),
});
export type ManagedUser = z.infer<typeof managedUser>;

/** Music Assistant's `AuthToken`: one sign-in. */
export const authToken = z.looseObject({
  token_id: z.string(),
  user_id: z.string(),
  name: z.string(),
  created_at: z.string(),
  expires_at: z.nullish(z.string()),
  last_used_at: z.nullish(z.string()),
  is_long_lived: z.boolean(),
});
export type AuthToken = z.infer<typeof authToken>;
