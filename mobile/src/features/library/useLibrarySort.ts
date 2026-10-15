import type { LibraryKind } from '@bopsie/client/api/endpoints/library';
import { create } from 'zustand';
import { sqliteKeyValueStore } from '@/auth/platformStores';
import { chosenSort, type SortOption } from './sorts';

const storageKey = (kind: LibraryKind) => `library.sort.${kind}`;

/** The order chosen for each kind this session; the store remembers it across launches. */
const useChosen = create<Partial<Record<LibraryKind, string>>>(() => ({}));

/** The order the library lists `kind` in. */
export function useLibrarySort(kind: LibraryKind): SortOption {
  const key = useChosen((chosen) => chosen[kind]);
  return chosenSort(kind, key ?? sqliteKeyValueStore.get(storageKey(kind)));
}

export function chooseLibrarySort(kind: LibraryKind, key: string) {
  sqliteKeyValueStore.set(storageKey(kind), key);
  useChosen.setState({ [kind]: key });
}
