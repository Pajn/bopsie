import { type ReactNode, useState } from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { Icon, type IconName } from './Icon';
import { Text, type WebPress } from './primitives';

/**
 * A button that is an icon, named for screen readers and in a tooltip by `label`. `primary`
 * fills it with the accent, as for play; `ghost` leaves it bare until hovered.
 */
export function IconButton({
  icon,
  label,
  onPress,
  variant = 'secondary',
  size = 'md',
  disabled,
  active,
}: {
  icon: IconName;
  label: string;
  onPress: () => void;
  variant?: 'primary' | 'secondary' | 'ghost';
  size?: 'sm' | 'md' | 'lg';
  disabled?: boolean;
  /** Switched on, as shuffle can be: drawn in the accent. */
  active?: boolean;
}) {
  const { theme } = useUnistyles();
  const color =
    variant === 'primary'
      ? theme.colors.onAccent
      : active
        ? theme.colors.accent
        : theme.colors.text;
  return (
    <Pressable
      onPress={onPress}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{
        disabled: disabled ?? false,
        ...(active !== undefined && { checked: active }),
      }}
      // A tooltip, as an icon alone does not say what it does.
      {...({ title: label } as object)}
      style={({ pressed, hovered }: WebPress) => [
        styles.button(variant, size),
        hovered && !disabled && styles.hovered(variant),
        pressed && styles.pressed,
        disabled && styles.disabled,
      ]}
    >
      <Icon name={icon} size={ICON[size]} color={color} />
    </Pressable>
  );
}

const ICON = { sm: 14, md: 16, lg: 20 } as const;
const BOX = { sm: 28, md: 34, lg: 44 } as const;

/**
 * A volume from 0 to 100 as a slider, with the mute switch beside it and the level after it.
 * The level shown follows the thumb while dragged; `onChange` hears it once let go.
 */
export function VolumeSlider({
  value,
  muted,
  label,
  onChange,
  onMute,
}: {
  value: number | null;
  muted?: boolean | null;
  label: string;
  onChange: (level: number) => void;
  onMute?: (muted: boolean) => void;
}) {
  const { theme } = useUnistyles();
  const [dragging, setDragging] = useState<number | null>(null);
  const shown = dragging ?? value ?? 0;
  const unavailable = value === null;
  const icon: IconName = muted || shown === 0 ? 'volume-x' : shown < 50 ? 'volume-1' : 'volume-2';
  return (
    <View style={styles.volume}>
      {onMute ? (
        <IconButton
          icon={icon}
          label={muted ? `Unmute ${label}` : `Mute ${label}`}
          variant="ghost"
          size="sm"
          disabled={unavailable}
          active={muted ?? false}
          onPress={() => onMute(!muted)}
        />
      ) : (
        <Icon name={icon} size={14} color={theme.colors.textMuted} />
      )}
      <View style={styles.track}>
        <input
          type="range"
          min={0}
          max={100}
          step={1}
          value={shown}
          disabled={unavailable}
          aria-label={`Volume of ${label}`}
          onChange={(event) => setDragging(Number(event.currentTarget.value))}
          onPointerUp={(event) => {
            onChange(Number(event.currentTarget.value));
            setDragging(null);
          }}
          onKeyUp={(event) => {
            onChange(Number(event.currentTarget.value));
            setDragging(null);
          }}
          style={{
            width: '100%',
            accentColor: theme.colors.accent,
            opacity: muted || unavailable ? 0.5 : 1,
            cursor: unavailable ? 'default' : 'pointer',
          }}
        />
      </View>
      <Text variant="caption" muted style={styles.level}>
        {unavailable ? '–' : Math.round(shown)}
      </Text>
    </View>
  );
}

/**
 * A section that opens on request, closed by default: for what only some want to see, such
 * as the raw data behind a page. On its own it is a card; `inline`, it is a section of one.
 */
export function Disclosure({
  title,
  hint,
  children,
  initiallyOpen = false,
  inline = false,
}: {
  title: string;
  hint?: string;
  children: ReactNode;
  initiallyOpen?: boolean;
  inline?: boolean;
}) {
  const { theme } = useUnistyles();
  const [open, setOpen] = useState(initiallyOpen);
  return (
    <View style={!inline && styles.disclosure}>
      <Pressable
        onPress={() => setOpen(!open)}
        accessibilityRole="button"
        accessibilityState={{ expanded: open }}
        style={({ hovered }: WebPress) => [
          inline ? styles.inlineSummary : styles.summary,
          hovered && styles.summaryHover,
        ]}
      >
        <Icon
          name={open ? 'chevron-down' : 'chevron-right'}
          size={16}
          color={theme.colors.textMuted}
        />
        <Text variant={inline ? 'label' : 'heading'} muted={inline} style={styles.grow}>
          {inline ? title.toUpperCase() : title}
        </Text>
        {hint ? (
          <Text variant="caption" muted>
            {hint}
          </Text>
        ) : null}
      </Pressable>
      {open ? <View style={inline ? styles.inlineDetails : styles.details}>{children}</View> : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  button: (variant: 'primary' | 'secondary' | 'ghost', size: 'sm' | 'md' | 'lg') => ({
    width: BOX[size],
    height: BOX[size],
    borderRadius: size === 'lg' ? BOX.lg / 2 : theme.radius.md,
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor:
      variant === 'primary'
        ? theme.colors.accent
        : variant === 'secondary'
          ? theme.colors.surfaceVariant
          : 'transparent',
  }),
  hovered: (variant: 'primary' | 'secondary' | 'ghost') => ({
    backgroundColor: variant === 'primary' ? theme.colors.accent : theme.colors.border,
    opacity: variant === 'primary' ? 0.9 : 1,
  }),
  pressed: { opacity: 0.7 },
  disabled: { opacity: 0.4 },
  volume: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.sm, minWidth: 0 },
  track: { flex: 1, minWidth: 80 },
  level: { width: 28, textAlign: 'right', fontVariant: ['tabular-nums'] },
  disclosure: {
    backgroundColor: theme.colors.surface,
    borderColor: theme.colors.border,
    borderWidth: 1,
    borderRadius: theme.radius.lg,
    overflow: 'hidden',
  },
  summary: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.sm,
    paddingHorizontal: theme.spacing.lg,
    paddingVertical: theme.spacing.md,
  },
  inlineSummary: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.xs,
    marginHorizontal: -theme.spacing.xs,
    paddingHorizontal: theme.spacing.xs,
    paddingVertical: theme.spacing.xs,
    borderRadius: theme.radius.md,
  },
  summaryHover: { backgroundColor: theme.colors.surfaceVariant },
  grow: { flex: 1 },
  details: { padding: theme.spacing.lg, paddingTop: 0 },
  inlineDetails: { paddingTop: theme.spacing.sm },
}));
