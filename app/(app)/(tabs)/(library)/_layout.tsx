import { Stack } from 'expo-router';
import { useUnistyles } from 'react-native-unistyles';
import { stackScreenOptions } from '@/ui/stackOptions';

/** The library tab's own stack, so detail pages keep the tab bar and the mini player. */
export default function LibraryStackLayout() {
  const { theme } = useUnistyles();
  return (
    <Stack screenOptions={stackScreenOptions(theme)}>
      <Stack.Screen name="index" options={{ headerShown: false }} />
      {/* Detail screens set their title once they know it; until then the header stays blank. */}
      <Stack.Screen name="album/[id]" options={{ title: '' }} />
      <Stack.Screen name="artist/[id]" options={{ title: '' }} />
      <Stack.Screen name="playlist/[id]" options={{ title: '' }} />
    </Stack>
  );
}
