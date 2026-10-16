import { Link, Slot, usePathname } from 'expo-router';
import { type ReactNode, useState } from 'react';
import { Pressable, type StyleProp, View, type ViewStyle } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useAccountActions } from '@/auth/AuthProvider';
import { ConnectionProvider, useConnection } from '@/connection/ConnectionProvider';
import { Icon, type IconName } from '@/ui/Icon';
import { Text, type WebPress } from '@/ui/primitives';

const PAGES: ReadonlyArray<{ href: string; title: string; icon: IconName }> = [
  { href: '/players', title: 'Players', icon: 'speaker' },
  { href: '/queues', title: 'Queues', icon: 'list' },
  { href: '/library', title: 'Library', icon: 'disc' },
  { href: '/logs', title: 'Logs', icon: 'terminal' },
  { href: '/users', title: 'Users', icon: 'users' },
  { href: '/server', title: 'Server', icon: 'server' },
];

/**
 * A link to a page that shows when the pointer is over it, keeping its hover state itself.
 */
function NavLink({
  href,
  label,
  active,
  style,
  hoverStyle,
  children,
}: {
  href: string;
  label: string;
  active: boolean;
  style: StyleProp<ViewStyle>;
  hoverStyle?: StyleProp<ViewStyle>;
  children: ReactNode;
}) {
  const [hovered, setHovered] = useState(false);
  return (
    <Link href={href as never} asChild>
      <Pressable
        accessibilityRole="link"
        accessibilityLabel={label}
        accessibilityState={{ selected: active }}
        onHoverIn={() => setHovered(true)}
        onHoverOut={() => setHovered(false)}
      >
        {/* Styled inside: the link merges its child's style, which loses unistyles' classes. */}
        <View style={[style, hovered && hoverStyle]}>{children}</View>
      </Pressable>
    </Link>
  );
}

/** Whether the connection is up, as a coloured dot and, for screen readers, words. */
function Status() {
  const { status } = useConnection();
  const { theme } = useUnistyles();
  const color =
    status === 'ready'
      ? theme.colors.good
      : status === 'connecting' || status === 'reconnecting'
        ? theme.colors.warning
        : theme.colors.danger;
  return (
    <View style={styles.status} accessibilityLabel={`Connection ${status}`}>
      <View style={[styles.dot, { backgroundColor: color }]} />
      {status === 'ready' ? null : (
        <Text variant="caption" muted>
          {status}
        </Text>
      )}
    </View>
  );
}

function SignOut({ compact }: { compact?: boolean }) {
  const { account } = useConnection();
  const { removeAccount } = useAccountActions();
  const { theme } = useUnistyles();
  return (
    <Pressable
      onPress={() => void removeAccount(account.id)}
      accessibilityRole="button"
      accessibilityLabel="Sign out"
      style={({ hovered, pressed }: WebPress) => [
        styles.signOut,
        (hovered || pressed) && styles.navItemHover,
      ]}
    >
      <Icon name="log-out" size={16} color={theme.colors.textMuted} />
      {compact ? null : (
        <Text muted style={styles.grow}>
          Sign out
        </Text>
      )}
    </Pressable>
  );
}

/** The desktop's navigation: a column of pages, the account and the connection below. */
function Sidebar({ pathname }: { pathname: string }) {
  const { account } = useConnection();
  const { theme } = useUnistyles();
  return (
    <View style={styles.sidebar}>
      <View style={styles.brand}>
        <Text variant="heading">Bopsie</Text>
        <Status />
      </View>
      <View style={styles.navItems}>
        {PAGES.map((page) => {
          const active = pathname.startsWith(page.href);
          return (
            <NavLink
              key={page.href}
              href={page.href}
              label={page.title}
              active={active}
              style={[styles.navItem, active && styles.navItemActive]}
              hoverStyle={styles.navItemHover}
            >
              <Icon
                name={page.icon}
                size={17}
                color={active ? theme.colors.accent : theme.colors.textMuted}
              />
              <Text style={active ? styles.navTextActive : undefined}>{page.title}</Text>
            </NavLink>
          );
        })}
      </View>
      <View style={styles.account}>
        <Text variant="caption" muted numberOfLines={1}>
          Signed in as {account.username}
        </Text>
        <SignOut />
      </View>
    </View>
  );
}

/** The phone's navigation: the app and the connection above, every page as a tab below. */
function TopBar() {
  return (
    <View style={styles.topBar}>
      <Text variant="heading" style={styles.grow}>
        Bopsie
      </Text>
      <Status />
      <SignOut compact />
    </View>
  );
}

function TabBar({ pathname }: { pathname: string }) {
  const { theme } = useUnistyles();
  const insets = useSafeAreaInsets();
  return (
    <View style={[styles.tabBar, { paddingBottom: Math.max(insets.bottom, theme.spacing.xs) }]}>
      {PAGES.map((page) => {
        const active = pathname.startsWith(page.href);
        const color = active ? theme.colors.accent : theme.colors.textMuted;
        return (
          // The link's own element takes no style, so a wrapper shares out the width.
          <View key={page.href} style={styles.tabSlot}>
            <NavLink href={page.href} label={page.title} active={active} style={styles.tab}>
              <Icon name={page.icon} size={20} color={color} />
              <Text variant="caption" style={{ color }}>
                {page.title}
              </Text>
            </NavLink>
          </View>
        );
      })}
    </View>
  );
}

function Shell() {
  const pathname = usePathname();
  return (
    <View style={styles.shell}>
      <View style={styles.desktopOnly}>
        <Sidebar pathname={pathname} />
      </View>
      <View style={styles.column}>
        <View style={styles.phoneOnly}>
          <TopBar />
        </View>
        <View style={styles.main}>
          <Slot />
        </View>
        <View style={styles.phoneOnly}>
          <TabBar pathname={pathname} />
        </View>
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
  shell: { flex: 1, flexDirection: 'row', backgroundColor: theme.colors.background },
  desktopOnly: { display: { xs: 'none', md: 'flex' } },
  phoneOnly: { display: { xs: 'flex', md: 'none' } },
  column: { flex: 1, minWidth: 0 },
  main: { flex: 1, minHeight: 0 },
  grow: { flex: 1 },
  sidebar: {
    width: 232,
    height: '100%',
    padding: theme.spacing.md,
    gap: theme.spacing.lg,
    backgroundColor: theme.colors.surface,
    borderRightWidth: 1,
    borderColor: theme.colors.border,
  },
  brand: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: theme.spacing.sm,
    paddingTop: theme.spacing.sm,
  },
  navItems: { flex: 1, gap: 2 },
  navItem: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingHorizontal: theme.spacing.sm,
    paddingVertical: theme.spacing.sm,
    borderRadius: theme.radius.md,
  },
  navItemHover: { backgroundColor: theme.colors.surfaceVariant },
  navItemActive: { backgroundColor: theme.colors.surfaceVariant },
  navTextActive: { color: theme.colors.accent, fontWeight: '600' },
  account: {
    gap: theme.spacing.xs,
    paddingTop: theme.spacing.md,
    borderTopWidth: 1,
    borderColor: theme.colors.border,
  },
  signOut: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    padding: theme.spacing.sm,
    borderRadius: theme.radius.md,
  },
  status: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.xs },
  dot: { width: 8, height: 8, borderRadius: 4 },
  topBar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.sm,
    paddingHorizontal: theme.spacing.lg,
    paddingVertical: theme.spacing.sm,
    minHeight: 52,
    backgroundColor: theme.colors.surface,
    borderBottomWidth: 1,
    borderColor: theme.colors.border,
  },
  tabBar: {
    flexDirection: 'row',
    paddingTop: theme.spacing.xs,
    backgroundColor: theme.colors.surface,
    borderTopWidth: 1,
    borderColor: theme.colors.border,
  },
  tabSlot: { flex: 1, minWidth: 0 },
  tab: { alignItems: 'center', gap: 2, paddingVertical: theme.spacing.xs },
}));
