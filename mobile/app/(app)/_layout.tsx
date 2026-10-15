import { Stack } from 'expo-router';
import { useUnistyles } from 'react-native-unistyles';
import { ConnectionProvider } from '@/connection/ConnectionProvider';
import { ActiveGroupProvider } from '@/features/outputs/ActiveGroupProvider';
import { RemoteSession } from '@/features/player/RemoteSession';
import { modalScreenOptions, sheetScreenOptions, stackScreenOptions } from '@/ui/stackOptions';

export default function AppLayout() {
  const { theme } = useUnistyles();
  return (
    <ConnectionProvider>
      <ActiveGroupProvider>
        <RemoteSession />
        <Stack screenOptions={stackScreenOptions(theme)}>
          <Stack.Screen name="(tabs)" options={{ headerShown: false }} />
          <Stack.Screen name="outputs" options={{ title: 'Play on', ...modalScreenOptions }} />
          <Stack.Screen name="player-menu" options={sheetScreenOptions} />
          <Stack.Screen name="item-menu" options={sheetScreenOptions} />
          <Stack.Screen name="library-sort" options={sheetScreenOptions} />
          <Stack.Screen name="calibrate" options={{ title: 'Speaker sync' }} />
        </Stack>
      </ActiveGroupProvider>
    </ConnectionProvider>
  );
}
