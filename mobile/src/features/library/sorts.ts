import type { LibraryKind } from '@bopsie/client/api/endpoints/library';

/** An order the library can be listed in, by the server's `order_by` key. */
export interface SortOption {
  key: string;
  label: string;
}

const BY_NAME: SortOption = { key: 'sort_name', label: 'Name' };
const RECENTLY_ADDED: SortOption = { key: 'timestamp_added_desc', label: 'Recently added' };

/** The orders a kind can be listed in, the default first. */
export function sortOptions(kind: LibraryKind): SortOption[] {
  return kind === 'albums'
    ? [
        BY_NAME,
        RECENTLY_ADDED,
        { key: 'year_desc', label: 'Newest' },
        { key: 'year', label: 'Oldest' },
      ]
    : [BY_NAME, RECENTLY_ADDED];
}

/** The chosen order, or the default when nothing, or something no longer offered, was chosen. */
export function chosenSort(kind: LibraryKind, key: string | null): SortOption {
  const options = sortOptions(kind);
  return options.find((option) => option.key === key) ?? BY_NAME;
}
