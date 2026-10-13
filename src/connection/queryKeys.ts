/** Every cached query is scoped by account, so switching accounts never shows another's data. */
export const queryKeys = {
  library: (accountId: string) => ['library', accountId] as const,
  libraryList: (accountId: string, kind: string, search: string) =>
    ['library', accountId, 'list', kind, search] as const,
  item: (accountId: string, kind: string, itemId: string, provider: string) =>
    ['library', accountId, kind, provider, itemId] as const,
  search: (accountId: string, query: string) => ['library', accountId, 'search', query] as const,
  queues: (accountId: string) => ['queue', accountId] as const,
  queue: (accountId: string, queueId: string) => ['queue', accountId, queueId] as const,
  queueItems: (accountId: string, queueId: string) =>
    ['queue', accountId, queueId, 'items'] as const,
  players: (accountId: string) => ['players', accountId] as const,
};
