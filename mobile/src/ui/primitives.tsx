import type { ReactNode } from 'react';
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
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';

export function Screen({ children, style, ...props }: ViewProps) {
  const insets = useSafeAreaInsets();
  return (
    <View style={[styles.screen, { paddingTop: insets.top }, style]} {...props}>
      {children}
    </View>
  );
}

type Variant = 'title' | 'heading' | 'body' | 'label' | 'caption';

export function Text({
  variant = 'body',
  muted,
  style,
  ...props
}: TextProps & { variant?: Variant; muted?: boolean }) {
  return <RNText style={[styles.text(variant), muted && styles.muted, style]} {...props} />;
}

export function TextField({
  label,
  error,
  style,
  ...props
}: TextInputProps & { label: string; error?: string }) {
  const { theme } = useUnistyles();
  return (
    <View style={styles.field}>
      <Text variant="label" muted>
        {label}
      </Text>
      <TextInput
        placeholderTextColor={theme.colors.textMuted}
        style={[styles.input, error ? styles.inputError : null, style]}
        {...props}
      />
      {error ? (
        <Text variant="caption" style={styles.errorText}>
          {error}
        </Text>
      ) : null}
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
          color={variant === 'primary' ? theme.colors.onAccent : theme.colors.accent}
        />
      ) : (
        <Text style={styles.buttonText(variant)}>{title}</Text>
      )}
    </Pressable>
  );
}

export function InlineError({ message, testID }: { message: string | null; testID?: string }) {
  if (!message) return null;
  return (
    <Text variant="body" style={styles.errorText} testID={testID}>
      {message}
    </Text>
  );
}

export function Stack({
  gap = 'md',
  children,
  style,
  ...props
}: ViewProps & { gap?: 'sm' | 'md' | 'lg' | 'xl'; children: ReactNode }) {
  return (
    <View style={[styles.stack(gap), style]} {...props}>
      {children}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  screen: {
    flex: 1,
    backgroundColor: theme.colors.background,
    paddingHorizontal: theme.spacing.lg,
  },
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
    paddingVertical: theme.spacing.sm + 2,
  },
  inputError: { borderColor: theme.colors.danger },
  errorText: { color: theme.colors.danger },
  button: (variant: 'primary' | 'secondary') => ({
    alignItems: 'center',
    justifyContent: 'center',
    minHeight: 48,
    borderRadius: theme.radius.md,
    paddingHorizontal: theme.spacing.lg,
    backgroundColor: variant === 'primary' ? theme.colors.accent : theme.colors.surfaceVariant,
  }),
  buttonText: (variant: 'primary' | 'secondary') => ({
    ...theme.typography.body,
    fontWeight: '600',
    color: variant === 'primary' ? theme.colors.onAccent : theme.colors.text,
  }),
  pressed: { opacity: 0.85 },
  disabled: { opacity: 0.5 },
  stack: (gap: 'sm' | 'md' | 'lg' | 'xl') => ({ gap: theme.spacing[gap] }),
}));
