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
import { Icon, type IconName } from './Icon';

/**
 * A page's frame: it scrolls, holds its content to a readable width, and leads with its title,
 * an optional line under it, and actions beside it. `back` names the page it sits under.
 * `fill` leaves the scrolling to the content, as a log does. `hero` takes the title's place,
 * for a page that leads with more than words, as an album's does with its cover.
 */
export function Page({
  title,
  subtitle,
  back,
  actions,
  fill,
  wide,
  hero,
  children,
}: {
  title: string;
  subtitle?: ReactNode;
  back?: { href: string; label: string };
  actions?: ReactNode;
  fill?: boolean;
  /** Up to the wider width, for tables and grids. */
  wide?: boolean;
  hero?: ReactNode;
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
      {hero ?? (
        <View style={styles.titleRow}>
          <View style={styles.titleText}>
            <Text variant="title" numberOfLines={2}>
              {title}
            </Text>
            {typeof subtitle === 'string' ? <Text muted>{subtitle}</Text> : subtitle}
          </View>
          {actions ? <View style={styles.actions}>{actions}</View> : null}
        </View>
      )}
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
      {label ? (
        <Text variant="label" muted>
          {label}
        </Text>
      ) : null}
      <TextInput
        placeholderTextColor={theme.colors.textMuted}
        // The label is drawn above; this names the field for a screen reader too.
        {...(label && { accessibilityLabel: label })}
        style={[styles.input, style]}
        {...props}
      />
    </View>
  );
}

type ButtonVariant = 'primary' | 'secondary' | 'danger';

export function Button({
  title,
  icon,
  loading,
  variant = 'primary',
  disabled,
  ...props
}: Omit<PressableProps, 'children'> & {
  title: string;
  icon?: IconName;
  loading?: boolean;
  /** `danger` for what cannot be undone, as deleting. */
  variant?: ButtonVariant;
}) {
  const { theme } = useUnistyles();
  const inactive = disabled || loading;
  return (
    <Pressable
      accessibilityRole="button"
      disabled={inactive}
      style={({ pressed, hovered }: WebPress) => [
        styles.button(variant),
        hovered && !inactive && styles.buttonHover(variant),
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
        <>
          {icon ? (
            <Icon
              name={icon}
              size={15}
              color={
                variant === 'primary'
                  ? theme.colors.onAccent
                  : variant === 'danger'
                    ? theme.colors.danger
                    : theme.colors.text
              }
            />
          ) : null}
          <Text style={styles.buttonText(variant)}>{title}</Text>
        </>
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

/**
 * A button that opens a list to pick one of `options` from. The list is the browser's own, a
 * select drawn over the button, so it opens over the page and as a phone's picker.
 */
export function Choice({
  title,
  label,
  options,
  onChoose,
  disabled,
}: {
  title: string;
  /** What it chooses, for a screen reader, where the title alone does not say: `Plays on`. */
  label?: string;
  options: Array<{ value: string; label: string }>;
  onChoose: (value: string) => void;
  disabled?: boolean;
}) {
  const { theme } = useUnistyles();
  const [hovered, setHovered] = useState(false);
  const inactive = disabled || options.length === 0;
  return (
    <View
      style={[
        styles.button('secondary'),
        hovered && !inactive && styles.buttonHover('secondary'),
        inactive && styles.disabled,
      ]}
    >
      <Text style={styles.buttonText('secondary')} numberOfLines={1}>
        {title}
      </Text>
      <Icon name="chevron-down" size={14} color={theme.colors.textMuted} />
      <select
        aria-label={label ?? title}
        value=""
        disabled={inactive}
        onChange={(event) => {
          if (event.currentTarget.value) onChoose(event.currentTarget.value);
        }}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        style={{
          position: 'absolute',
          inset: 0,
          width: '100%',
          height: '100%',
          opacity: 0,
          cursor: inactive ? 'default' : 'pointer',
          font: 'inherit',
        }}
      >
        {/* What the list is for, not an option: the title can name one, as the current player. */}
        <option value="" disabled>
          {label ?? title}
        </option>
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
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
  button: (variant: ButtonVariant) => ({
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: theme.spacing.xs,
    minHeight: 34,
    borderRadius: theme.radius.md,
    paddingHorizontal: theme.spacing.md,
    backgroundColor:
      variant === 'primary'
        ? theme.colors.accent
        : variant === 'danger'
          ? 'transparent'
          : theme.colors.surfaceVariant,
    borderWidth: variant === 'danger' ? 1 : 0,
    borderColor: theme.colors.danger,
  }),
  buttonText: (variant: ButtonVariant) => ({
    ...theme.typography.body,
    fontWeight: '600',
    color:
      variant === 'primary'
        ? theme.colors.onAccent
        : variant === 'danger'
          ? theme.colors.danger
          : theme.colors.text,
  }),
  buttonHover: (variant: ButtonVariant) => ({
    backgroundColor: variant === 'primary' ? theme.colors.accent : theme.colors.border,
    opacity: variant === 'primary' ? 0.9 : 1,
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
  // The label beside its value, narrower on a phone; a long value wraps in its own column.
  fact: { flexDirection: 'row', gap: theme.spacing.md },
  factLabel: { width: { xs: 112, md: 140 }, paddingTop: 2 },
  factValue: { flex: 1, minWidth: 0 },
}));

type ToneColors = Record<'good' | 'warning' | 'danger' | 'textMuted' | 'accent', string>;

function toneColor(theme: { colors: ToneColors }, tone: Tone): string {
  return tone === 'muted' ? theme.colors.textMuted : theme.colors[tone];
}
