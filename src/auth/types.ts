/** A signed-in user on a Music Assistant server. Everything user-specific is scoped by `id`. */
export interface Account {
  /** Stable per server and user, so signing in again replaces rather than duplicates. */
  id: string;
  serverUrl: string;
  /** The server's own id, from its server info. */
  serverId: string;
  userId: string;
  username: string;
  addedAt: number;
}

export function accountId(serverId: string, userId: string): string {
  return `${serverId}#${userId}`;
}
