import type { Tabs } from 'expo-router';
import type { ComponentProps } from 'react';
import { Pressable, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { StyleSheet } from 'react-native-unistyles';
import { ConnectionBanner } from '@/connection/ConnectionBanner';
import { Text } from '@/ui/primitives';
import { setTabBarHeight, useDockHeight } from './sheetStore';
import { useReportedHeight } from './useReportedHeight';

/** Derived from Expo Router's `Tabs` so no navigation package needs importing directly. */
export type TabBarProps = Parameters<NonNullable<ComponentProps<typeof Tabs>['tabBar']>>[0];

/**
 * The tab bar, with room above it for the mini player.
 *
 * The mini player is drawn by the player sheet, which carries it up the screen and so cannot
 * sit inside the bar. What is left here is a gap the same size, which keeps a screen's content
 * clear of it: the navigator insets screens by what this measures.
 */
export function TabBar({ state, descriptors, navigation }: TabBarProps) {
  const insets = useSafeAreaInsets();
  const dockHeight = useDockHeight();
  const measure = useReportedHeight(setTabBarHeight);
  return (
    <View style={styles.shell} ref={measure.ref} onLayout={measure.onLayout}>
      <View style={{ height: dockHeight }} />
      <ConnectionBanner />
      <View style={[styles.bar, { paddingBottom: Math.max(insets.bottom, 8) }]}>
        {state.routes.map((route, index) => {
          const options = descriptors[route.key]?.options;
          if (!options) return null;
          const focused = state.index === index;
          const label = typeof options.title === 'string' ? options.title : route.name;
          const icon = options.tabBarIcon?.({
            focused,
            color: focused ? styles.active.color : styles.inactive.color,
            size: 22,
          });
          return (
            <Pressable
              key={route.key}
              accessibilityRole="button"
              accessibilityState={focused ? { selected: true } : {}}
              accessibilityLabel={options.tabBarAccessibilityLabel ?? label}
              testID={options.tabBarButtonTestID}
              onPress={() => {
                const event = navigation.emit({
                  type: 'tabPress',
                  target: route.key,
                  canPreventDefault: true,
                });
                if (!focused && !event.defaultPrevented)
                  navigation.navigate(route.name, route.params);
              }}
              style={styles.tab}
            >
              {icon}
              <Text variant="caption" style={focused ? styles.active : styles.inactive}>
                {label}
              </Text>
            </Pressable>
          );
        })}
      </View>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  shell: { backgroundColor: theme.colors.surface },
  bar: {
    flexDirection: 'row',
    backgroundColor: theme.colors.surface,
    borderTopWidth: 1,
    borderTopColor: theme.colors.border,
    paddingTop: theme.spacing.sm,
  },
  tab: { flex: 1, alignItems: 'center', gap: 2 },
  active: { color: theme.colors.accent },
  inactive: { color: theme.colors.textMuted },
}));
