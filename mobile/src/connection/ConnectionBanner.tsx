import { useEffect, useState } from 'react';
import { ActivityIndicator, Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { Collapsible, useOpenProgress } from '@/ui/Collapsible';
import { Icon } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { useConnection } from './ConnectionProvider';

/** How long the server may be away before the banner says so, so a blip passes unremarked. */
const GRACE_MS = 1500;

/**
 * Says when the server cannot be reached: while the connection retries, and once it has given
 * up, with a way to try again. What the app shows meanwhile is what it last heard.
 */
export function ConnectionBanner() {
  const { connection, status, account } = useConnection();
  const { theme } = useUnistyles();
  const away = status === 'reconnecting' || status === 'closed';
  const [shown, setShown] = useState(false);
  useEffect(() => {
    if (!away) {
      setShown(false);
      return;
    }
    const timer = setTimeout(() => setShown(true), GRACE_MS);
    return () => clearTimeout(timer);
  }, [away]);
  const progress = useOpenProgress(shown);
  const host = hostOf(account.serverUrl);
  const gaveUp = status === 'closed';

  return (
    <Collapsible progress={progress} open={shown}>
      <View style={styles.banner} accessibilityLiveRegion="polite" testID="connection-banner">
        {gaveUp ? (
          <Icon name="wifi-off" size={16} color={theme.colors.textMuted} />
        ) : (
          <ActivityIndicator size="small" color={theme.colors.textMuted} />
        )}
        <Text variant="caption" muted style={styles.text} numberOfLines={1}>
          {gaveUp ? `Can't reach ${host}` : `Reconnecting to ${host}…`}
        </Text>
        {gaveUp ? (
          <Pressable
            onPress={() => connection.retry()}
            hitSlop={12}
            accessibilityRole="button"
            testID="connection-retry"
          >
            <Text variant="label" style={styles.action}>
              Try again
            </Text>
          </Pressable>
        ) : null}
      </View>
    </Collapsible>
  );
}

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

const styles = StyleSheet.create((theme) => ({
  banner: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.sm,
    paddingHorizontal: theme.spacing.lg,
    paddingVertical: theme.spacing.sm,
    backgroundColor: theme.colors.surfaceVariant,
  },
  text: { flex: 1 },
  action: { color: theme.colors.accent },
}));
