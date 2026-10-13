import { Tabs } from 'expo-router';
import type { ColorValue } from 'react-native';
import { useUnistyles } from 'react-native-unistyles';
import { TabBar } from '@/features/player/TabBar';
import { Icon, type IconName } from '@/ui/Icon';

function icon(name: IconName) {
  return ({ color, size }: { color: ColorValue; size: number }) => (
    <Icon name={name} color={color} size={size} />
  );
}

export default function TabsLayout() {
  const { theme } = useUnistyles();
  return (
    <Tabs
      tabBar={(props) => <TabBar {...props} />}
      screenOptions={{
        headerShown: false,
        sceneStyle: { backgroundColor: theme.colors.background },
      }}
    >
      <Tabs.Screen
        name="(library)"
        options={{ title: 'Library', tabBarIcon: icon('disc'), tabBarButtonTestID: 'tab-library' }}
      />
      <Tabs.Screen
        name="(search)"
        options={{ title: 'Search', tabBarIcon: icon('search'), tabBarButtonTestID: 'tab-search' }}
      />
      <Tabs.Screen
        name="account"
        options={{ title: 'Account', tabBarIcon: icon('user'), tabBarButtonTestID: 'tab-account' }}
      />
    </Tabs>
  );
}
