import type { LibraryKind } from '@bopsie/client/api/endpoints/library';
import { create } from 'zustand';
import { sqliteKeyValueStore } from '@/auth/platformStores';
import { chosenSort, type SortOption } from './sorts';

const storageKey = (kind: LibraryKind) => `library.sort.${kind}`;

/** The order chosen for each kind this session; the store remembers it across launches, as it
 * does the layout. */
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

export type LibraryLayout = 'list' | 'grid';

const layoutKey = (kind: LibraryKind) => `library.layout.${kind}`;
const useLayouts = create<Partial<Record<LibraryKind, LibraryLayout>>>(() => ({}));

/** Whether the library lists `kind` as rows or a grid; tracks are always rows. */
export function useLibraryLayout(kind: LibraryKind): LibraryLayout {
  const chosen = useLayouts((layouts) => layouts[kind]);
  if (kind === 'tracks') return 'list';
  return chosen ?? (sqliteKeyValueStore.get(layoutKey(kind)) === 'grid' ? 'grid' : 'list');
}

export function chooseLibraryLayout(kind: LibraryKind, layout: LibraryLayout) {
  sqliteKeyValueStore.set(layoutKey(kind), layout);
  useLayouts.setState({ [kind]: layout });
}
