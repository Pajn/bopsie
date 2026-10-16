import { Link } from 'expo-router';
import { type ReactNode, useState } from 'react';
import {
  ActivityIndicator,
  Pressable,
  type PressableProps,
  Text as RNText,
  ScrollView,
  TextInput,
  type TextInputProps,
  type TextProps,
  View,
  type ViewProps,
} from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';

/**
 * A page's frame: it scrolls, holds its content to a readable width, and leads with its title,
 * an optional line under it, and actions beside it. `back` names the page it sits under.
 * `fill` leaves the scrolling to the content, as a log does.
 */
export function Page({
  title,
  subtitle,
  back,
  actions,
  fill,
  wide,
  children,
}: {
  title: string;
  subtitle?: ReactNode;
  back?: { href: string; label: string };
  actions?: ReactNode;
  fill?: boolean;
  /** Up to the wider width, for tables and grids. */
  wide?: boolean;
  children: ReactNode;
}) {
  const header = (
    <View style={styles.pageHeader}>
      {back ? (
        <Link href={back.href as never} asChild>
          <Pressable accessibilityRole="link" style={styles.back}>
            <Text variant="label" style={styles.backText}>
              ‹ {back.label}
            </Text>
          </Pressable>
        </Link>
      ) : null}
      <View style={styles.titleRow}>
        <View style={styles.titleText}>
          <Text variant="title" numberOfLines={2}>
            {title}
          </Text>
          {typeof subtitle === 'string' ? <Text muted>{subtitle}</Text> : subtitle}
        </View>
        {actions ? <View style={styles.actions}>{actions}</View> : null}
      </View>
    </View>
  );
  if (fill) {
    return (
      <View style={[styles.page(wide ?? false), styles.fill]}>
        {header}
        <View style={styles.fill}>{children}</View>
      </View>
    );
  }
  return (
    <ScrollView contentContainerStyle={styles.scroll}>
      <View style={styles.page(wide ?? false)}>
        {header}
        <View style={styles.pageBody}>{children}</View>
      </View>
    </ScrollView>
  );
}

/** What a Pressable's style is given on the web, where it also knows the pointer is over it. */
export type WebPress = { pressed: boolean; hovered?: boolean };

type Variant = 'title' | 'heading' | 'body' | 'label' | 'caption' | 'mono';
type Gap = 'xs' | 'sm' | 'md' | 'lg' | 'xl';

export function Text({
  variant = 'body',
  muted,
  style,
  ...props
}: TextProps & { variant?: Variant; muted?: boolean }) {
  return <RNText style={[styles.text(variant), muted && styles.muted, style]} {...props} />;
}

export function TextField({ label, style, ...props }: TextInputProps & { label: string }) {
  const { theme } = useUnistyles();
  return (
    <View style={styles.field}>
      <Text variant="label" muted>
        {label}
      </Text>
      <TextInput
        placeholderTextColor={theme.colors.textMuted}
        style={[styles.input, style]}
        {...props}
      />
    </View>
  );
}

export function Button({
  title,
  loading,
  variant = 'primary',
  disabled,
  ...props
}: Omit<PressableProps, 'children'> & {
  title: string;
  loading?: boolean;
  variant?: 'primary' | 'secondary';
}) {
  const { theme } = useUnistyles();
  const inactive = disabled || loading;
  return (
    <Pressable
      accessibilityRole="button"
      disabled={inactive}
      style={({ pressed }) => [
        styles.button(variant),
        pressed && styles.pressed,
        inactive && styles.disabled,
      ]}
      {...props}
    >
      {loading ? (
        <ActivityIndicator
          size="small"
          color={variant === 'primary' ? theme.colors.onAccent : theme.colors.accent}
        />
      ) : (
        <Text style={styles.buttonText(variant)}>{title}</Text>
      )}
    </Pressable>
  );
}

export function InlineError({ message }: { message: string | null }) {
  if (!message) return null;
  return <Text style={styles.errorText}>{message}</Text>;
}

export function Stack({
  gap = 'md',
  horizontal,
  children,
  style,
  ...props
}: ViewProps & { gap?: Gap; horizontal?: boolean; children: ReactNode }) {
  return (
    <View style={[styles.stack(gap), horizontal && styles.horizontal, style]} {...props}>
      {children}
    </View>
  );
}

/** A bordered panel with an optional heading. */
export function Card({
  title,
  children,
  style,
  ...props
}: ViewProps & { title?: string; children: ReactNode }) {
  return (
    <View style={[styles.card, style]} {...props}>
      {title ? <Text variant="heading">{title}</Text> : null}
      {children}
    </View>
  );
}

/** Rows of a label and its value. */
export function Facts({ rows }: { rows: Array<[string, ReactNode]> }) {
  return (
    <View style={styles.facts}>
      {rows.map(([label, value]) => (
        <View key={label} style={styles.fact}>
          <Text variant="label" muted style={styles.factLabel}>
            {label}
          </Text>
          {typeof value === 'string' || typeof value === 'number' ? (
            <Text style={styles.factValue}>{value}</Text>
          ) : (
            <View style={styles.factValue}>{value}</View>
          )}
        </View>
      ))}
    </View>
  );
}

export type Tone = 'good' | 'warning' | 'danger' | 'muted' | 'accent';

/** A short label in a coloured outline, for a state or a kind. */
export function Badge({ label, tone = 'muted' }: { label: string; tone?: Tone }) {
  return (
    <View style={styles.badge(tone)}>
      <Text variant="caption" style={styles.badgeText(tone)}>
        {label}
      </Text>
    </View>
  );
}

/** A button that opens a list to pick one of `options` from. */
export function Choice({
  title,
  options,
  onChoose,
  disabled,
}: {
  title: string;
  options: Array<{ value: string; label: string }>;
  onChoose: (value: string) => void;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  return (
    <View>
      <Button
        title={open ? `${title} ▴` : `${title} ▾`}
        variant="secondary"
        disabled={disabled || options.length === 0}
        onPress={() => setOpen(!open)}
      />
      {open ? (
        <View style={styles.menu}>
          {options.map((option) => (
            <Pressable
              key={option.value}
              accessibilityRole="menuitem"
              style={({ pressed }) => [styles.menuItem, pressed && styles.pressed]}
              onPress={() => {
                setOpen(false);
                onChoose(option.value);
              }}
            >
              <Text>{option.label}</Text>
            </Pressable>
          ))}
        </View>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  scroll: { flexGrow: 1 },
  fill: { flex: 1, minHeight: 0 },
  page: (wide: boolean) => ({
    width: '100%',
    maxWidth: wide ? 1280 : 960,
    alignSelf: 'center',
    paddingHorizontal: { xs: theme.spacing.lg, md: theme.spacing.xl },
    paddingTop: { xs: theme.spacing.lg, md: theme.spacing.xl },
    paddingBottom: theme.spacing.xxl,
  }),
  pageHeader: { gap: theme.spacing.xs, marginBottom: theme.spacing.lg },
  back: { alignSelf: 'flex-start' },
  backText: { color: theme.colors.accent },
  titleRow: {
    flexDirection: 'row',
    alignItems: 'flex-end',
    gap: theme.spacing.md,
    flexWrap: 'wrap',
  },
  titleText: { flex: 1, minWidth: 200, gap: 2 },
  actions: { flexDirection: 'row', gap: theme.spacing.sm, flexWrap: 'wrap' },
  pageBody: { gap: theme.spacing.lg },
  badge: (tone: Tone) => ({
    borderWidth: 1,
    borderColor: toneColor(theme, tone),
    borderRadius: theme.radius.full,
    paddingHorizontal: theme.spacing.sm,
    paddingVertical: 1,
    alignSelf: 'flex-start',
  }),
  badgeText: (tone: Tone) => ({ color: toneColor(theme, tone) }),
  menu: {
    marginTop: theme.spacing.xs,
    backgroundColor: theme.colors.surface,
    borderColor: theme.colors.border,
    borderWidth: 1,
    borderRadius: theme.radius.md,
    overflow: 'hidden',
  },
  menuItem: { paddingHorizontal: theme.spacing.md, paddingVertical: theme.spacing.sm },
  text: (variant: Variant) => ({ ...theme.typography[variant], color: theme.colors.text }),
  muted: { color: theme.colors.textMuted },
  field: { gap: theme.spacing.xs },
  input: {
    ...theme.typography.body,
    color: theme.colors.text,
    backgroundColor: theme.colors.surface,
    borderColor: theme.colors.border,
    borderWidth: 1,
    borderRadius: theme.radius.md,
    paddingHorizontal: theme.spacing.md,
    paddingVertical: theme.spacing.sm,
  },
  errorText: { ...theme.typography.body, color: theme.colors.danger },
  button: (variant: 'primary' | 'secondary') => ({
    alignItems: 'center',
    justifyContent: 'center',
    minHeight: 34,
    borderRadius: theme.radius.md,
    paddingHorizontal: theme.spacing.md,
    backgroundColor: variant === 'primary' ? theme.colors.accent : theme.colors.surfaceVariant,
  }),
  buttonText: (variant: 'primary' | 'secondary') => ({
    ...theme.typography.body,
    fontWeight: '600',
    color: variant === 'primary' ? theme.colors.onAccent : theme.colors.text,
  }),
  pressed: { opacity: 0.85 },
  disabled: { opacity: 0.5 },
  stack: (gap: Gap) => ({ gap: theme.spacing[gap] }),
  horizontal: { flexDirection: 'row', alignItems: 'center', flexWrap: 'wrap' },
  card: {
    gap: theme.spacing.md,
    padding: theme.spacing.lg,
    backgroundColor: theme.colors.surface,
    borderColor: theme.colors.border,
    borderWidth: 1,
    borderRadius: theme.radius.lg,
  },
  facts: { gap: theme.spacing.xs },
  // Side by side where there is room, the label above its value on a phone.
  fact: {
    flexDirection: { xs: 'column', md: 'row' },
    gap: { xs: 0, md: theme.spacing.md },
    paddingVertical: { xs: 2, md: 0 },
  },
  factLabel: { width: { xs: 'auto', md: 180 }, paddingTop: { xs: 0, md: 2 } },
  factValue: { flex: { xs: 0, md: 1 }, minWidth: 0 },
}));

type ToneColors = Record<'good' | 'warning' | 'danger' | 'textMuted' | 'accent', string>;

function toneColor(theme: { colors: ToneColors }, tone: Tone): string {
  return tone === 'muted' ? theme.colors.textMuted : theme.colors[tone];
}
