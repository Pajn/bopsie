import type { Stack } from 'expo-router';
import type { ComponentProps } from 'react';
import { Platform } from 'react-native';
import type { AppTheme } from './theme';

type NativeStackNavigationOptions = NonNullable<ComponentProps<typeof Stack.Screen>['options']>;

/**
 * Options shared by every native stack in the app.
 *
 * On Android the platform's default transition fades the new screen in while
 * shifting the old one aside, and the strip it uncovers is the window
 * background rather than either screen. The slide keeps the two screens
 * edge to edge, so nothing behind them shows. iOS keeps its own push, which
 * the interactive back gesture relies on.
 */
export function stackScreenOptions(theme: AppTheme): NativeStackNavigationOptions {
  return {
    headerStyle: { backgroundColor: theme.colors.background },
    headerTintColor: theme.colors.text,
    headerShadowVisible: false,
    headerBackButtonDisplayMode: 'minimal',
    contentStyle: { backgroundColor: theme.colors.background },
    animation: Platform.OS === 'android' ? 'slide_from_right' : 'default',
  };
}

/**
 * A screen that presents like a sheet: it rises over the current screen and
 * leaves it in place. iOS gets the platform's card modal; Android has no
 * modal transition of its own, so the rise is spelled out.
 */
export const modalScreenOptions: NativeStackNavigationOptions = {
  presentation: 'modal',
  animation: Platform.OS === 'android' ? 'slide_from_bottom' : 'default',
};

/**
 * A screen that takes the whole display, like the reader: it rises over the
 * current screen with no header of its own.
 */
export const fullScreenOptions: NativeStackNavigationOptions = {
  presentation: 'fullScreenModal',
  headerShown: false,
  animation: Platform.OS === 'android' ? 'slide_from_bottom' : 'default',
};
