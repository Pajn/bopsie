import type { ImageColors } from 'react-native-true-image';
import { contrast, mix } from '@/ui/color';

/** The colours the full player takes from its artwork. */
export interface PlayerTint {
  background: string;
  /** Raised things on the background, such as the play button. */
  surface: string;
  accent: string;
}

/** Text on the tinted background keeps at least this contrast, which body text asks for. */
const TEXT_CONTRAST = 7;
/** An accent stands out from the background by at least this, as controls must. */
const ACCENT_CONTRAST = 3;

/**
 * The player's colours for a cover: its background tinted toward the cover's darker colours
 * in the dark scheme and its lighter ones in the light, held off far enough for `text` to stay
 * readable, and an accent from the cover's vivid colours when one stands out from that
 * background, else the theme's. Null without colours, as when the cover could not be read.
 */
export function playerTint(
  colors: ImageColors | null | undefined,
  scheme: 'light' | 'dark',
  theme: { background: string; text: string; accent: string },
): PlayerTint | null {
  if (!colors) return null;
  const dark = scheme === 'dark';
  const base = dark
    ? (colors.darkMuted ?? colors.darkVibrant ?? colors.dominant)
    : (colors.lightMuted ?? colors.lightVibrant ?? colors.dominant);
  let background = mix(theme.background, base, dark ? 0.55 : 0.35);
  // Toward black, or white, until the text reads well on it.
  for (let step = 0; step < 10 && contrast(theme.text, background) < TEXT_CONTRAST; step++) {
    background = mix(background, dark ? '#000000' : '#FFFFFF', 0.2);
  }
  const candidates = dark
    ? [colors.lightVibrant, colors.vibrant, colors.lightMuted]
    : [colors.darkVibrant, colors.vibrant, colors.darkMuted];
  const accent =
    candidates.find(
      (candidate): candidate is string =>
        candidate != null && contrast(candidate, background) >= ACCENT_CONTRAST,
    ) ?? theme.accent;
  return { background, surface: mix(background, theme.text, 0.08), accent };
}
