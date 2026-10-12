/**
 * Design tokens. Themes are plain runtime objects so a cover-derived accent or
 * an e-ink profile can be produced by copying a base theme and overriding a
 * few fields.
 */
const spacing = { xs: 4, sm: 8, md: 12, lg: 16, xl: 24, xxl: 32 } as const;
const radius = { sm: 6, md: 12, lg: 20, full: 999 } as const;

const typography = {
  title: { fontSize: 28, fontWeight: '700', lineHeight: 34 },
  heading: { fontSize: 20, fontWeight: '600', lineHeight: 26 },
  body: { fontSize: 16, fontWeight: '400', lineHeight: 22 },
  label: { fontSize: 13, fontWeight: '500', lineHeight: 18 },
  caption: { fontSize: 12, fontWeight: '400', lineHeight: 16 },
} as const;

export const lightTheme = {
  name: 'light',
  colors: {
    background: '#FAFAFC',
    surface: '#FFFFFF',
    surfaceVariant: '#EEEEF2',
    text: '#1B1B1F',
    textMuted: '#5F5F68',
    accent: '#6750A4',
    onAccent: '#FFFFFF',
    border: '#D8D8DE',
    danger: '#B3261E',
  },
  spacing,
  radius,
  typography,
  isEink: false,
} as const;

export const darkTheme = {
  ...lightTheme,
  name: 'dark',
  colors: {
    background: '#121216',
    surface: '#1B1B1F',
    surfaceVariant: '#26262C',
    text: '#E6E1E5',
    textMuted: '#A6A4AB',
    accent: '#D0BCFF',
    onAccent: '#381E72',
    border: '#3A3A42',
    danger: '#F2B8B5',
  },
} as const;

/** Monochrome, high-contrast, no translucency. Refined later; exists so nothing assumes color. */
export const einkTheme = {
  ...lightTheme,
  name: 'eink',
  colors: {
    background: '#FFFFFF',
    surface: '#FFFFFF',
    surfaceVariant: '#F2F2F2',
    text: '#000000',
    textMuted: '#333333',
    accent: '#000000',
    onAccent: '#FFFFFF',
    border: '#000000',
    danger: '#000000',
  },
  isEink: true,
} as const;

export type AppTheme = typeof lightTheme | typeof darkTheme | typeof einkTheme;
