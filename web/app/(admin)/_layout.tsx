import { Link, Slot, usePathname } from 'expo-router';
import { Pressable, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useAccountActions } from '@/auth/AuthProvider';
import { ConnectionProvider, useConnection } from '@/connection/ConnectionProvider';
import { Button, Stack, Text } from '@/ui/primitives';

const PAGES = [
  { href: '/players', title: 'Players' },
  { href: '/queues', title: 'Queues' },
  { href: '/library', title: 'Library' },
  { href: '/logs', title: 'Logs' },
  { href: '/server', title: 'Server' },
] as const;

function Shell() {
  const { account, status } = useConnection();
  const { removeAccount } = useAccountActions();
  const pathname = usePathname();
  return (
    <View style={styles.shell}>
      <View style={styles.nav}>
        <Stack gap="xs">
          <Text variant="heading">Bopsie</Text>
          <Text variant="caption" muted>
            {account.username} · {status}
          </Text>
        </Stack>
        <Stack gap="xs" style={styles.links}>
          {PAGES.map((page) => (
            // A Link is not themed by unistyles; the Text inside it is.
            <Link key={page.href} href={page.href} asChild>
              <Pressable>
                <Text style={styles.link(pathname.startsWith(page.href))}>{page.title}</Text>
              </Pressable>
            </Link>
          ))}
        </Stack>
        <Button
          title="Sign out"
          variant="secondary"
          onPress={() => void removeAccount(account.id)}
        />
      </View>
      <View style={styles.main}>
        <Slot />
      </View>
    </View>
  );
}

export default function AdminLayout() {
  return (
    <ConnectionProvider>
      <Shell />
    </ConnectionProvider>
  );
}

const styles = StyleSheet.create((theme) => ({
  shell: {
    flex: 1,
    flexDirection: { xs: 'column', md: 'row' },
    backgroundColor: theme.colors.background,
  },
  nav: {
    width: { xs: '100%', md: 200 },
    gap: theme.spacing.lg,
    padding: theme.spacing.lg,
    borderColor: theme.colors.border,
    borderRightWidth: { xs: 0, md: 1 },
    borderBottomWidth: { xs: 1, md: 0 },
  },
  links: { flex: { xs: 0, md: 1 } },
  link: (active: boolean) => ({
    ...theme.typography.body,
    color: active ? theme.colors.accent : theme.colors.text,
    fontWeight: active ? '600' : '400',
    paddingVertical: theme.spacing.xs,
  }),
  main: { flex: 1, padding: theme.spacing.lg },
}));
