import {
  CommandError,
  NetworkError,
  SchemaError,
  SetupRequiredError,
} from '@bopsie/client/api/errors';
import { SignInError } from '@bopsie/client/auth/signIn';

/** A short message suitable for showing to the user. */
export function describeError(error: unknown): string {
  if (error instanceof SignInError || error instanceof SetupRequiredError) return error.message;
  if (error instanceof NetworkError) return 'Could not reach the server.';
  if (error instanceof CommandError) {
    if (error.isUnauthorized) return 'The server no longer accepts this sign-in.';
    return error.message;
  }
  if (error instanceof SchemaError) return 'The server answered in an unexpected way.';
  if (error instanceof Error) return error.message;
  return 'Something went wrong.';
}
