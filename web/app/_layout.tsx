import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Stack } from 'expo-router';
import { AuthProvider, useAccounts } from '@/auth/AuthProvider';

// The browser's own controls, such as a choice's menu and a slider, follow the system's light or
// dark as the theme does.
if (typeof document !== 'undefined') document.documentElement.style.colorScheme = 'light dark';

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: 5_000, retry: 1 } },
});

function RootNavigator() {
  const signedIn = useAccounts(
    (state) =>
      state.status === 'signedIn' &&
      state.activeAccountId !== null &&
      !state.expiredAccountIds.includes(state.activeAccountId),
  );
  const loading = useAccounts((state) => state.status === 'loading');
  if (loading) return null;
  return (
    <Stack screenOptions={{ headerShown: false }}>
      <Stack.Protected guard={signedIn}>
        <Stack.Screen name="(admin)" />
      </Stack.Protected>
      <Stack.Protected guard={!signedIn}>
        <Stack.Screen name="sign-in" />
      </Stack.Protected>
    </Stack>
  );
}

export default function RootLayout() {
  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <RootNavigator />
      </AuthProvider>
    </QueryClientProvider>
  );
}
