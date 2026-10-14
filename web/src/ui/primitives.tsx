import { type ReactNode, useState } from 'react';
import {
  ActivityIndicator,
  Pressable,
  type PressableProps,
  Text as RNText,
  TextInput,
  type TextInputProps,
  type TextProps,
  View,
  type ViewProps,
} from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';

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
  fact: { flexDirection: 'row', gap: theme.spacing.md },
  factLabel: { width: 160, paddingTop: 2 },
  factValue: { flex: 1 },
}));

type ToneColors = Record<'good' | 'warning' | 'danger' | 'textMuted' | 'accent', string>;

function toneColor(theme: { colors: ToneColors }, tone: Tone): string {
  return tone === 'muted' ? theme.colors.textMuted : theme.colors[tone];
}
