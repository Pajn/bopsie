/**
 * Design tokens, on Bopsie's palette but denser: this is a management view on a desktop
 * browser first, a phone second.
 */
const spacing = { xs: 4, sm: 8, md: 12, lg: 16, xl: 24, xxl: 32 } as const;
const radius = { sm: 4, md: 8, lg: 12, full: 999 } as const;

const typography = {
  title: { fontSize: 22, fontWeight: '700', lineHeight: 28 },
  heading: { fontSize: 16, fontWeight: '600', lineHeight: 22 },
  body: { fontSize: 14, fontWeight: '400', lineHeight: 20 },
  label: { fontSize: 12, fontWeight: '500', lineHeight: 16 },
  caption: { fontSize: 12, fontWeight: '400', lineHeight: 16 },
  mono: { fontSize: 12, fontWeight: '400', lineHeight: 18, fontFamily: 'ui-monospace, monospace' },
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
    warning: '#8A5A00',
    good: '#1E6B34',
    code: { string: '#2E7D32', value: '#6750A4' },
  },
  spacing,
  radius,
  typography,
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
    warning: '#F0C46C',
    good: '#8FD6A0',
    code: { string: '#A5D6A7', value: '#D0BCFF' },
  },
} as const;
