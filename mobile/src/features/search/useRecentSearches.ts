import { useCallback, useState } from 'react';
import { sqliteKeyValueStore } from '@/auth/platformStores';
import { useConnection } from '@/connection/ConnectionProvider';
import { remember } from './searchResults';

const storageKey = (accountId: string) => `search.recent.${accountId}`;

function load(accountId: string): string[] {
  try {
    const stored = JSON.parse(sqliteKeyValueStore.get(storageKey(accountId)) ?? '[]');
    return Array.isArray(stored) ? stored.filter((q): q is string => typeof q === 'string') : [];
  } catch {
    return [];
  }
}

/** The account's recent searches, newest first, kept across launches. */
export function useRecentSearches() {
  const { account } = useConnection();
  const [recent, setRecent] = useState(() => load(account.id));
  const save = useCallback(
    (next: string[]) => {
      sqliteKeyValueStore.set(storageKey(account.id), JSON.stringify(next));
      setRecent(next);
    },
    [account.id],
  );
  return {
    recent,
    add: useCallback(
      (query: string) => save(remember(load(account.id), query)),
      [account.id, save],
    ),
    remove: useCallback(
      (query: string) => save(load(account.id).filter((past) => past !== query)),
      [account.id, save],
    ),
    clear: useCallback(() => save([]), [save]),
  };
}
