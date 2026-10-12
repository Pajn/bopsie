import { CommandError, NetworkError, SchemaError, SetupRequiredError } from '@/api/errors';
import { SignInError } from '@/auth/signIn';

/** A short message suitable for showing to the user. */
export function describeError(error: unknown): string {
  if (error instanceof SignInError || error instanceof SetupRequiredError) return error.message;
  if (error instanceof NetworkError)
    return 'Could not reach the server. Check the address and your connection.';
  if (error instanceof CommandError) {
    if (error.isUnauthorized) return 'The server no longer accepts this sign-in.';
    if (error.code === 10) return 'This phone is not connected to Music Assistant as a player yet.';
    return error.message;
  }
  if (error instanceof SchemaError) return 'The server answered in an unexpected way.';
  if (error instanceof Error) return error.message;
  return 'Something went wrong.';
}
