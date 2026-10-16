import { type ReactNode, useState } from 'react';
import { Pressable, ScrollView, TextInput, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { duration } from '@/lib/time';
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
  mark,
}: {
  icon: IconName;
  label: string;
  onPress: () => void;
  variant?: 'primary' | 'secondary' | 'ghost';
  size?: 'sm' | 'md' | 'lg';
  disabled?: boolean;
  /** Switched on, as shuffle can be: drawn in the accent. */
  active?: boolean;
  /** A character at the icon's corner, as the 1 of repeating one track. */
  mark?: string;
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
      {mark ? <Text style={[styles.mark, { color }]}>{mark}</Text> : null}
    </Pressable>
  );
}

const ICON = { sm: 14, md: 16, lg: 20 } as const;
const BOX = { sm: 28, md: 34, lg: 44 } as const;
/** On a phone, room for a finger: about the 44 points touch guidelines ask for. */
const TOUCH = { sm: 40, md: 44, lg: 48 } as const;

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
 * Where in a track a player is, as a slider between the time played and the length. The time
 * shown follows the thumb while dragged; `onSeek` hears it once let go. Without a length, as
 * for a stream, it shows the time alone.
 */
export function SeekBar({
  elapsed,
  length,
  onSeek,
}: {
  elapsed: number;
  length: number;
  onSeek: (seconds: number) => void;
}) {
  const { theme } = useUnistyles();
  const [dragging, setDragging] = useState<number | null>(null);
  const shown = Math.min(dragging ?? elapsed, length || Number.POSITIVE_INFINITY);
  const seek = (value: string) => {
    onSeek(Number(value));
    setDragging(null);
  };
  return (
    <View style={styles.seek}>
      <Text variant="caption" muted style={styles.time}>
        {duration(shown)}
      </Text>
      <View style={styles.track}>
        {length > 0 ? (
          <input
            type="range"
            min={0}
            max={Math.round(length)}
            step={1}
            value={Math.round(shown)}
            aria-label="Position"
            onChange={(event) => setDragging(Number(event.currentTarget.value))}
            onPointerUp={(event) => seek(event.currentTarget.value)}
            onKeyUp={(event) => seek(event.currentTarget.value)}
            style={{ width: '100%', accentColor: theme.colors.accent, cursor: 'pointer' }}
          />
        ) : null}
      </View>
      <Text variant="caption" muted style={[styles.time, styles.timeEnd]}>
        {length > 0 ? duration(length) : ''}
      </Text>
    </View>
  );
}

/**
 * A row of tabs that picks one of `tabs`, scrolling sideways where they do not fit. `after`
 * starts a second group, set apart, as for tools beside what is browsed.
 */
export function Tabs<T extends string>({
  tabs,
  after = [],
  value,
  onChange,
}: {
  tabs: ReadonlyArray<{ value: T; label: string }>;
  after?: ReadonlyArray<{ value: T; label: string }>;
  value: T;
  onChange: (value: T) => void;
}) {
  const tab = (t: { value: T; label: string }) => (
    <Pressable
      key={t.value}
      onPress={() => onChange(t.value)}
      accessibilityRole="tab"
      accessibilityState={{ selected: t.value === value }}
      style={({ hovered }: WebPress) => [
        styles.tab(t.value === value),
        hovered && t.value !== value && styles.tabHover,
      ]}
    >
      <Text style={styles.tabText(t.value === value)}>{t.label}</Text>
    </Pressable>
  );
  return (
    <ScrollView horizontal showsHorizontalScrollIndicator={false} accessibilityRole="tablist">
      <View style={styles.tabs}>
        {tabs.map(tab)}
        {after.length > 0 ? <View style={styles.tabDivider} /> : null}
        {after.map(tab)}
      </View>
    </ScrollView>
  );
}

/** A field to search or filter with, marked with a magnifier and named by `label`. */
export function SearchField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (text: string) => void;
}) {
  const { theme } = useUnistyles();
  return (
    <View style={styles.search}>
      <Icon name="search" size={16} color={theme.colors.textMuted} />
      <TextInput
        value={value}
        onChangeText={onChange}
        placeholder={label}
        placeholderTextColor={theme.colors.textMuted}
        autoCorrect={false}
        accessibilityLabel={label}
        style={styles.searchInput}
      />
      {value ? (
        <IconButton icon="x" label="Clear" variant="ghost" size="sm" onPress={() => onChange('')} />
      ) : null}
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
    width: { xs: TOUCH[size], md: BOX[size] },
    height: { xs: TOUCH[size], md: BOX[size] },
    borderRadius: size === 'lg' ? 9999 : theme.radius.md,
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
  mark: { position: 'absolute', top: 2, right: 4, fontSize: 9, fontWeight: '700' },
  disabled: { opacity: 0.4 },
  volume: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.sm, minWidth: 0 },
  track: { flex: 1, minWidth: 80 },
  level: { width: 28, textAlign: 'right', fontVariant: ['tabular-nums'] },
  seek: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.sm, minWidth: 0 },
  time: { width: 44, fontVariant: ['tabular-nums'] },
  timeEnd: { textAlign: 'right' },
  search: {
    flex: 1,
    minWidth: 220,
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.sm,
    paddingLeft: theme.spacing.md,
    paddingRight: theme.spacing.xs,
    minHeight: 38,
    borderRadius: theme.radius.md,
    borderWidth: 1,
    borderColor: theme.colors.border,
    backgroundColor: theme.colors.surface,
  },
  searchInput: {
    ...theme.typography.body,
    flex: 1,
    minWidth: 0,
    paddingVertical: theme.spacing.sm,
    color: theme.colors.text,
  },
  tabs: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.xs },
  tab: (selected: boolean) => ({
    paddingHorizontal: theme.spacing.md,
    paddingVertical: theme.spacing.xs + 2,
    borderRadius: theme.radius.full,
    backgroundColor: selected ? theme.colors.text : 'transparent',
  }),
  tabHover: { backgroundColor: theme.colors.surfaceVariant },
  tabText: (selected: boolean) => ({
    fontWeight: '600',
    color: selected ? theme.colors.background : theme.colors.textMuted,
  }),
  tabDivider: {
    width: 1,
    height: 20,
    marginHorizontal: theme.spacing.sm,
    backgroundColor: theme.colors.border,
  },
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
