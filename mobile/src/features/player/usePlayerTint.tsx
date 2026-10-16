import { imageKey, imageUrl } from '@bopsie/client/api/images';
import type { WithImages } from '@bopsie/client/api/schemas/media';
import { useQuery } from '@tanstack/react-query';
import { createContext, type ReactNode, useContext } from 'react';
import { Image } from 'react-native-true-image';
import { useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { type PlayerTint, playerTint } from './coverPalette';

/**
 * The colours of the artwork of what plays, read once per cover from a small sample of it and
 * kept, and the player's tint from them for the scheme in use; null until read, for a cover
 * that cannot be, or in the e-ink theme.
 */
export function useCoverTint(artwork: WithImages | null): PlayerTint | null {
  const { account } = useConnection();
  const { theme } = useUnistyles();
  const url = artwork ? imageUrl(account.serverUrl, artwork, 256) : null;
  const colors = useQuery({
    queryKey: ['cover-colors', url],
    queryFn: () => {
      const cacheKey = artwork ? imageKey(artwork) : null;
      return url ? Image.getColors({ uri: url, ...(cacheKey && { cacheKey }) }) : null;
    },
    enabled: url !== null,
    staleTime: Number.POSITIVE_INFINITY,
  }).data;
  if (theme.isEink) return null;
  return playerTint(colors, theme.name === 'dark' ? 'dark' : 'light', theme.colors);
}

const TintContext = createContext<PlayerTint | null>(null);

export function PlayerTintProvider({
  tint,
  children,
}: {
  tint: PlayerTint | null;
  children: ReactNode;
}) {
  return <TintContext.Provider value={tint}>{children}</TintContext.Provider>;
}

/** The player's colours: the cover's tint where there is one, else the theme's. */
export function usePlayerColors() {
  const tint = useContext(TintContext);
  const { theme } = useUnistyles();
  return {
    background: tint?.background ?? theme.colors.background,
    surface: tint?.surface ?? theme.colors.surfaceVariant,
    accent: tint?.accent ?? theme.colors.accent,
  };
}
