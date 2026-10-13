import { Stack } from 'expo-router';
import { useUnistyles } from 'react-native-unistyles';
import { ConnectionProvider } from '@/connection/ConnectionProvider';
import { ActiveGroupProvider } from '@/features/outputs/ActiveGroupProvider';
import { modalScreenOptions, stackScreenOptions } from '@/ui/stackOptions';

export default function AppLayout() {
  const { theme } = useUnistyles();
  return (
    <ConnectionProvider>
      <ActiveGroupProvider>
        <Stack screenOptions={stackScreenOptions(theme)}>
          <Stack.Screen name="(tabs)" options={{ headerShown: false }} />
          <Stack.Screen name="player" options={{ title: 'Now playing', ...modalScreenOptions }} />
          <Stack.Screen name="outputs" options={{ title: 'Play on', ...modalScreenOptions }} />
        </Stack>
      </ActiveGroupProvider>
    </ConnectionProvider>
  );
}
