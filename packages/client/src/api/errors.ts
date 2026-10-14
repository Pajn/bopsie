/** Error codes from `music_assistant_models/errors.py` that the app treats specially. */
export const ErrorCode = {
  mediaNotFound: 2,
  loginFailed: 6,
  playerUnavailable: 10,
  invalidCommand: 12,
  authenticationRequired: 20,
  authenticationFailed: 21,
  insufficientPermissions: 22,
  invalidToken: 23,
} as const;

/** The server answered a command with an error frame. */
export class CommandError extends Error {
  readonly command: string;
  readonly code: number;

  constructor(command: string, code: number, details: string | undefined) {
    super(details || `${command} failed with error ${code}`);
    this.name = 'CommandError';
    this.command = command;
    this.code = code;
  }

  /** The token is missing, invalid or expired; signing in again is the only way forward. */
  get isUnauthorized(): boolean {
    return (
      this.code === ErrorCode.authenticationRequired ||
      this.code === ErrorCode.authenticationFailed ||
      this.code === ErrorCode.invalidToken
    );
  }
}

/** The connection failed or closed before the command was answered. */
export class NetworkError extends Error {
  readonly url: string;
  override readonly cause: unknown;

  constructor(url: string, cause: unknown) {
    super(`Connection to ${url} failed: ${cause instanceof Error ? cause.message : String(cause)}`);
    this.name = 'NetworkError';
    this.url = url;
    this.cause = cause;
  }
}

/** The server answered but the result did not match what we expect. */
export class SchemaError extends Error {
  readonly command: string;
  readonly issues: unknown;

  constructor(command: string, issues: unknown) {
    super(`The result of ${command} did not match the expected shape`);
    this.name = 'SchemaError';
    this.command = command;
    this.issues = issues;
  }
}

/** The server needs first-run setup in its web interface before anyone can sign in. */
export class SetupRequiredError extends Error {
  constructor() {
    super(
      'This Music Assistant server has not been set up yet. Finish setup in its web interface.',
    );
    this.name = 'SetupRequiredError';
  }
}
