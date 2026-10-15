import type { LibraryKind } from '@bopsie/client/api/endpoints/library';
import { useRouter } from 'expo-router';
import { MenuRow, MenuSheet, MenuTitle } from './menu';
import { sortOptions } from './sorts';
import { chooseLibrarySort, useLibrarySort } from './useLibrarySort';

/** The orders the library can list a kind in. Choosing one closes the menu. */
export function LibrarySortMenu({ kind }: { kind: LibraryKind }) {
  const router = useRouter();
  const current = useLibrarySort(kind);
  return (
    <MenuSheet testID="library-sort-menu">
      <MenuTitle title="Sort by" />
      {sortOptions(kind).map((option) => (
        <MenuRow
          key={option.key}
          label={option.label}
          selected={option.key === current.key}
          onPress={() => {
            chooseLibrarySort(kind, option.key);
            router.back();
          }}
          testID={`library-sort-${option.key}`}
        />
      ))}
    </MenuSheet>
  );
}
