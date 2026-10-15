import type { ImageColors } from 'react-native-true-image';
import { contrast } from '@/ui/color';
import { playerTint } from './coverPalette';

const light = { background: '#FAFAFC', text: '#1B1B1F', accent: '#6750A4' };
const dark = { background: '#121216', text: '#E6E1E5', accent: '#D0BCFF' };

const colors = (overrides: Partial<ImageColors> = {}): ImageColors => ({
  dominant: '#3A5F8A',
  vibrant: '#2E7BD6',
  lightVibrant: '#8EC1F5',
  darkVibrant: '#16406E',
  muted: '#5C7591',
  lightMuted: '#B4C3D3',
  darkMuted: '#2B3A4A',
  ...overrides,
});

describe('playerTint', () => {
  it('is null without colours', () => {
    expect(playerTint(null, 'dark', dark)).toBeNull();
  });

  it('keeps text readable on the tinted background in either scheme', () => {
    for (const [scheme, theme] of [
      ['dark', dark],
      ['light', light],
    ] as const) {
      const tint = playerTint(colors(), scheme, theme);
      expect(tint).not.toBeNull();
      expect(contrast(theme.text, tint?.background ?? '')).toBeGreaterThanOrEqual(7);
    }
  });

  it('even for a cover with only a bright colour, in the dark', () => {
    const tint = playerTint(
      colors({ darkMuted: null, darkVibrant: null, dominant: '#FFE14D' }),
      'dark',
      dark,
    );
    expect(contrast(dark.text, tint?.background ?? '')).toBeGreaterThanOrEqual(7);
  });

  it("takes an accent from the cover that stands out, else the theme's", () => {
    expect(playerTint(colors(), 'dark', dark)?.accent).toBe('#8EC1F5');
    const grey = '#777777';
    const tint = playerTint(
      {
        dominant: grey,
        vibrant: null,
        lightVibrant: null,
        darkVibrant: null,
        muted: grey,
        lightMuted: null,
        darkMuted: null,
      },
      'light',
      light,
    );
    expect(tint?.accent).toBe(light.accent);
  });
});
