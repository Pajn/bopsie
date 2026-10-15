import type { MusicProvider } from '@bopsie/client/api/schemas/media';

/** Where a search looks: everywhere, the library alone, or one catalog such as Spotify. */
export interface SearchScope {
  key: string;
  label: string;
  /** The providers to search, or undefined for every one. */
  providers: string[] | undefined;
}

export const EVERYWHERE: SearchScope = { key: 'all', label: 'All', providers: undefined };

/**
 * The scopes to choose from: none when the library is the only source, else everywhere, the
 * library, and each catalog. With one catalog, everywhere reads "Both".
 */
export function searchScopes(providers: MusicProvider[]): SearchScope[] {
  const catalogs = providers.filter((provider) => provider.is_streaming_provider);
  if (catalogs.length === 0) return [];
  return [
    { ...EVERYWHERE, label: catalogs.length === 1 ? 'Both' : 'All' },
    { key: 'library', label: 'Library', providers: ['library'] },
    ...catalogs.map((catalog) => ({
      key: catalog.instance_id,
      label: catalog.name,
      providers: [catalog.instance_id],
    })),
  ];
}

/** The name to mark an item from `provider` with: none for the library's own. */
export function sourceName(
  providers: MusicProvider[] | undefined,
  provider: string,
): string | null {
  if (provider === 'library') return null;
  return providers?.find((p) => p.instance_id === provider || p.domain === provider)?.name ?? null;
}
