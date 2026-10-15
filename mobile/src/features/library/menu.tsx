import type { WithImages } from '@bopsie/client/api/schemas/media';
import type { ReactNode } from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { Icon, type IconName } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { Artwork } from './Artwork';

/** A menu sheet, sized by its content: the sheet fits itself to it. */
export function MenuSheet({ children, testID }: { children: ReactNode; testID: string }) {
  return (
    <View style={styles.sheet} testID={testID}>
      {children}
    </View>
  );
}

/** What the menu is about, above its rows. */
export function MenuHeader({
  item,
  title,
  subtitle,
  round,
  placeholder,
}: {
  item: WithImages;
  title: string;
  subtitle?: string;
  round?: boolean;
  placeholder?: IconName;
}) {
  return (
    <View style={styles.header}>
      <Artwork
        item={item}
        size={48}
        imageSize={80}
        {...(round !== undefined && { round })}
        {...(placeholder && { placeholder })}
      />
      <View style={styles.text}>
        <Text numberOfLines={1}>{title}</Text>
        {subtitle ? (
          <Text variant="caption" muted numberOfLines={1}>
            {subtitle}
          </Text>
        ) : null}
      </View>
    </View>
  );
}

export function MenuRow({
  icon,
  label,
  detail,
  onPress,
  testID,
}: {
  icon: IconName;
  label: string;
  detail?: string;
  onPress: () => void;
  testID: string;
}) {
  const { theme } = useUnistyles();
  return (
    <Pressable
      onPress={onPress}
      style={({ pressed }) => [styles.row, pressed && styles.pressed]}
      accessibilityRole="button"
      accessibilityLabel={detail ? `${label}: ${detail}` : label}
      testID={testID}
    >
      <Icon name={icon} size={20} color={theme.colors.textMuted} />
      <View style={styles.text}>
        <Text>{label}</Text>
        {detail ? (
          <Text variant="caption" muted numberOfLines={1}>
            {detail}
          </Text>
        ) : null}
      </View>
    </Pressable>
  );
}

const styles = StyleSheet.create((theme, rt) => ({
  sheet: {
    backgroundColor: theme.colors.background,
    paddingTop: theme.spacing.lg,
    paddingBottom: theme.spacing.lg + rt.insets.bottom,
  },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingHorizontal: theme.spacing.lg,
    paddingBottom: theme.spacing.md,
    marginBottom: theme.spacing.xs,
    borderBottomWidth: StyleSheet.hairlineWidth,
    borderBottomColor: theme.colors.border,
  },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.lg,
    paddingHorizontal: theme.spacing.lg,
    paddingVertical: theme.spacing.md,
  },
  pressed: { opacity: 0.6 },
  text: { flex: 1, gap: 2 },
}));

/** The button that opens a menu: on a row, or in a page's header. */
export function MoreButton({
  onPress,
  label = 'More',
  vertical,
  testID,
}: {
  onPress: () => void;
  label?: string;
  vertical?: boolean;
  testID: string;
}) {
  const { theme } = useUnistyles();
  return (
    <Pressable
      onPress={onPress}
      hitSlop={12}
      style={({ pressed }) => pressed && styles.pressed}
      accessibilityRole="button"
      accessibilityLabel={label}
      testID={testID}
    >
      <Icon
        name={vertical ? 'more-vertical' : 'more-horizontal'}
        size={vertical ? 20 : 24}
        color={vertical ? theme.colors.textMuted : theme.colors.text}
      />
    </Pressable>
  );
}
