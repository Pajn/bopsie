import { StyleSheet } from 'react-native-unistyles';
import { darkTheme, einkTheme, lightTheme } from './theme';

const themes = { light: lightTheme, dark: darkTheme, eink: einkTheme };
const breakpoints = { xs: 0, sm: 400, md: 600, lg: 900 };

type AppThemes = typeof themes;
type AppBreakpoints = typeof breakpoints;

declare module 'react-native-unistyles' {
  export interface UnistylesThemes extends AppThemes {}
  export interface UnistylesBreakpoints extends AppBreakpoints {}
}

StyleSheet.configure({
  themes,
  breakpoints,
  settings: { adaptiveThemes: true },
});
