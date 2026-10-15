import { Tabs } from 'expo-router';
import { type ColorValue, View } from 'react-native';
import { useUnistyles } from 'react-native-unistyles';
import { PlayerSheet } from '@/features/player/PlayerSheet';
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
    // The player sheet covers the tabs rather than sitting in them: it is drawn from the mini
    // player above the bar right up over the screen.
    <View style={{ flex: 1 }}>
      <Tabs
        tabBar={(props) => <TabBar {...props} />}
        screenOptions={{
          headerShown: false,
          sceneStyle: { backgroundColor: theme.colors.background },
        }}
      >
        <Tabs.Screen
          name="(home)"
          options={{ title: 'Home', tabBarIcon: icon('home'), tabBarButtonTestID: 'tab-home' }}
        />
        <Tabs.Screen
          name="(library)"
          options={{
            title: 'Library',
            tabBarIcon: icon('disc'),
            tabBarButtonTestID: 'tab-library',
          }}
        />
        <Tabs.Screen
          name="(search)"
          options={{
            title: 'Search',
            tabBarIcon: icon('search'),
            tabBarButtonTestID: 'tab-search',
          }}
        />
        <Tabs.Screen
          name="account"
          options={{
            title: 'Account',
            tabBarIcon: icon('user'),
            tabBarButtonTestID: 'tab-account',
          }}
        />
      </Tabs>
      <PlayerSheet />
    </View>
  );
}
