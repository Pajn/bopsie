import type { WithImages } from '@bopsie/client/api/schemas/media';
import type { ActiveGroup, OtherOutput } from '@bopsie/client/outputs/outputs';
import Slider from '@react-native-community/slider';
import { type ReactNode, useEffect, useState } from 'react';
import { Platform, Pressable, ScrollView, View } from 'react-native';
import Animated, {
  FadeInDown,
  FadeOutDown,
  interpolateColor,
  useAnimatedStyle,
} from 'react-native-reanimated';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { Artwork } from '@/features/library/Artwork';
import { nowPlaying } from '@/features/player/nowPlaying';
import { useQueue } from '@/features/player/usePlayback';
import { usePresence } from '@/lib/presence';
import { Collapsible, Fold, useOpenProgress } from '@/ui/Collapsible';
import { Icon, type IconName } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { useActiveGroup } from './ActiveGroupProvider';
import { useOutputActions, useOutputs } from './useOutputs';

const UNDO_MS = 6000;
const NONE: OtherOutput[] = [];

/**
 * Where the music plays. The active group is on top with its volumes; every
 * other player or group opens to the three things it can do: join the active
 * group, take over its queue, or become the one the app controls.
 */
export function OutputsScreen() {
  const outputs = useOutputs();
  const { queueId: phoneId } = useConnection();
  const queue = useQueue().data;
  const [open, setOpen] = useState<string | null>(null);
  const [undo, setUndo] = useState<{ name: string; run: () => void } | null>(null);
  const listed = usePresence(outputs?.others ?? NONE, (other) => other.id);

  useEffect(() => {
    if (!undo) return;
    const timer = setTimeout(() => setUndo(null), UNDO_MS);
    return () => clearTimeout(timer);
  }, [undo]);

  if (!outputs) {
    return (
      <View style={styles.screen}>
        <Text muted style={styles.notice}>
          Looking for players…
        </Text>
      </View>
    );
  }

  const { active } = outputs;
  const hasQueue = (queue?.items ?? 0) > 0;
  const current = queue ? nowPlaying(queue) : null;

  return (
    <View style={styles.screen}>
      <ScrollView contentContainerStyle={styles.content} testID="outputs">
        {phoneId === null ? (
          <Text muted style={styles.notice}>
            This phone cannot play music itself yet. Choose a player to control.
          </Text>
        ) : null}
        {active ? (
          <ActiveCard
            active={active}
            title={current?.title ?? null}
            artwork={current?.artwork ?? null}
          />
        ) : null}
        {listed.shown.length > 0 ? (
          <Text variant="label" muted style={styles.section}>
            Other players
          </Text>
        ) : null}
        {/* Players joining the active group fold away from here, and those leaving it fold in. */}
        {listed.shown.map(({ key, item: other, present, arriving }) => (
          <Fold key={key} present={present} arriving={arriving} onLeft={() => listed.left(key)}>
            <OtherRow
              other={other}
              active={active}
              open={open === other.id}
              onToggle={() => setOpen(open === other.id ? null : other.id)}
              canMove={hasQueue}
              onMoved={(run) => {
                setOpen(null);
                setUndo({ name: other.name, run });
              }}
              onChosen={() => setOpen(null)}
            />
          </Fold>
        ))}
      </ScrollView>
      {undo ? (
        <Animated.View entering={FadeInDown} exiting={FadeOutDown} style={styles.toast}>
          <Text style={styles.toastText} numberOfLines={1}>
            Moved to {undo.name}
          </Text>
          <Pressable
            onPress={() => {
              undo.run();
              setUndo(null);
            }}
            hitSlop={12}
            accessibilityRole="button"
            testID="outputs-undo"
          >
            <Text style={styles.toastAction}>Undo</Text>
          </Pressable>
        </Animated.View>
      ) : null}
    </View>
  );
}

function ActiveCard({
  active,
  title,
  artwork,
}: {
  active: ActiveGroup;
  title: string | null;
  artwork: WithImages | null;
}) {
  const actions = useOutputActions();
  const { unconfirmed } = useActiveGroup();
  const grouped = active.members.length > 1;
  const members = usePresence(active.members, (member) => member.id);
  // The members show while the group has more than one, and fold away with the last but one.
  const membersShown = useOpenProgress(grouped);
  return (
    <View style={styles.card}>
      <View style={styles.row}>
        <Artwork
          item={artwork ?? { image: null }}
          size={44}
          imageSize={160}
          placeholder="speaker"
        />
        <View style={styles.grow}>
          <Text style={styles.strong} numberOfLines={1} testID="outputs-active-name">
            {active.name}
          </Text>
          <Text variant="caption" muted numberOfLines={1}>
            {!active.available ? 'Unavailable' : (title ?? 'Nothing playing')}
          </Text>
        </View>
      </View>
      {active.volume !== null ? (
        <Volume
          value={active.volume}
          label={grouped ? `Volume of ${active.name}` : 'Volume'}
          onChange={actions.setGroupVolume}
          testID="outputs-volume"
          // Lines the group's slider up with the members'.
          end={grouped ? <View style={styles.end} /> : null}
        />
      ) : null}
      <Collapsible progress={membersShown} open={grouped}>
        {members.shown.map(({ key, item: member, present, arriving }) => {
          const remove = member.removable ? (
            <Pressable
              onPress={() => actions.remove(member, active.members)}
              hitSlop={8}
              style={[styles.end, styles.remove]}
              accessibilityRole="button"
              accessibilityLabel={`Remove ${member.name} from the group`}
              testID={`member-remove-${member.id}`}
            >
              <Icon name="x" size={16} />
            </Pressable>
          ) : (
            <View style={styles.end} />
          );
          return (
            <Fold key={key} present={present} arriving={arriving} onLeft={() => members.left(key)}>
              <View
                style={[styles.member, unconfirmed.has(member.id) && styles.unconfirmed]}
                testID={`member-${member.id}`}
              >
                <View style={styles.memberName}>
                  <Text variant="label" style={styles.grow} numberOfLines={1}>
                    {member.name}
                  </Text>
                  {member.volume === null ? remove : null}
                </View>
                {member.volume !== null ? (
                  <Volume
                    value={member.volume}
                    label={`Volume of ${member.name}`}
                    onChange={(level) => actions.setVolume(member.id, level)}
                    testID={`member-volume-${member.id}`}
                    small
                    end={remove}
                  />
                ) : null}
              </View>
            </Fold>
          );
        })}
      </Collapsible>
    </View>
  );
}

/** A volume slider that holds its own value while dragged and sends it on release. */
function Volume({
  value,
  label,
  onChange,
  testID,
  small,
  end,
}: {
  value: number;
  label: string;
  onChange: (level: number) => void;
  testID: string;
  small?: boolean;
  /** Shown after the level, such as a member's remove button. */
  end?: ReactNode;
}) {
  const { theme } = useUnistyles();
  const [dragging, setDragging] = useState<number | null>(null);
  const shown = dragging ?? value;
  return (
    <View style={styles.volume}>
      <Slider
        style={styles.slider}
        minimumValue={0}
        maximumValue={100}
        step={1}
        value={shown}
        onSlidingStart={(next) => setDragging(next)}
        onValueChange={(next) => dragging !== null && setDragging(next)}
        onSlidingComplete={(next) => {
          setDragging(null);
          onChange(next);
        }}
        minimumTrackTintColor={theme.colors.accent}
        maximumTrackTintColor={theme.colors.border}
        thumbTintColor={small ? theme.colors.textMuted : theme.colors.accent}
        accessibilityLabel={label}
        testID={testID}
      />
      <Text variant="caption" muted style={styles.level}>
        {Math.round(shown)}
      </Text>
      {end}
    </View>
  );
}

function OtherRow({
  other,
  active,
  open,
  onToggle,
  canMove,
  onMoved,
  onChosen,
}: {
  other: OtherOutput;
  active: ActiveGroup | null;
  open: boolean;
  onToggle: () => void;
  canMove: boolean;
  onMoved: (undo: () => void) => void;
  onChosen: () => void;
}) {
  const actions = useOutputActions();
  const { unconfirmed } = useActiveGroup();
  const progress = useOpenProgress(open);
  const { theme } = useUnistyles();
  const tileColor = theme.colors.surfaceVariant;
  const cardTileColor = theme.colors.background;
  const icon: IconName = other.isPhone ? 'smartphone' : other.isGroup ? 'layers' : 'speaker';
  const what = other.isGroup ? 'group' : 'player';
  // Open, the row and its actions share a card, which fades in behind them.
  const cardStyle = useAnimatedStyle(() => ({ opacity: progress.value }));
  // On the card, the icon's tile turns light, as the actions' are.
  const tileStyle = useAnimatedStyle(() => ({
    backgroundColor: interpolateColor(progress.value, [0, 1], [tileColor, cardTileColor]),
  }));
  const chevronStyle = useAnimatedStyle(() => ({
    transform: [{ rotate: `${progress.value * 180}deg` }],
  }));
  return (
    <View style={[styles.otherCard, unconfirmed.has(other.id) && styles.unconfirmed]}>
      <Animated.View style={[styles.otherCardBackground, cardStyle]} />
      <Pressable
        onPress={onToggle}
        disabled={!other.available}
        style={({ pressed }) => [styles.row, styles.other, pressed && styles.pressed]}
        accessibilityRole="button"
        accessibilityState={{ expanded: open, disabled: !other.available }}
        testID={`output-${other.id}`}
      >
        <Animated.View style={[styles.icon, tileStyle, !other.available && styles.unavailable]}>
          <Icon name={icon} size={16} />
        </Animated.View>
        <View style={[styles.grow, !other.available && styles.unavailable]}>
          <Text style={open ? styles.strong : undefined} numberOfLines={1}>
            {other.name}
          </Text>
          <Text variant="caption" muted numberOfLines={1}>
            {other.status}
          </Text>
        </View>
        {other.available ? (
          <Animated.View style={chevronStyle}>
            <Icon name="chevron-down" size={18} />
          </Animated.View>
        ) : null}
      </Pressable>
      <Collapsible progress={progress} open={open}>
        <View style={styles.actions}>
          {active && other.canJoin ? (
            <Action
              icon="plus"
              title="Join this group"
              testID="output-join"
              onPress={() => {
                actions.join(other.id);
                onChosen();
              }}
            >
              {`${other.name} plays along with ${active.name}.`}
              {other.playing ? (
                <Text style={styles.warning}>{` ${other.playing} stops.`}</Text>
              ) : null}
            </Action>
          ) : null}
          {active && canMove ? (
            <Action
              icon="arrow-right"
              title="Move playback here"
              testID="output-move"
              onPress={() => {
                const undo = actions.move(other.id);
                if (undo) onMoved(undo);
              }}
            >
              {`The queue moves to ${other.name}. ${active.name} stops.`}
            </Action>
          ) : null}
          <Action
            icon="sliders"
            title={`Control this ${what}`}
            testID="output-control"
            onPress={() => {
              actions.control(other.id);
              onChosen();
            }}
          >
            Play, pause and change volume here. Nothing else stops.
          </Action>
        </View>
      </Collapsible>
    </View>
  );
}

function Action({
  icon,
  title,
  children,
  onPress,
  testID,
}: {
  icon: IconName;
  title: string;
  children: ReactNode;
  onPress: () => void;
  testID: string;
}) {
  return (
    <Pressable
      onPress={onPress}
      style={({ pressed }) => [styles.action, pressed && styles.pressed]}
      accessibilityRole="button"
      testID={testID}
    >
      <View style={[styles.icon, styles.actionIcon]}>
        <Icon name={icon} size={16} />
      </View>
      <View style={styles.grow}>
        <Text style={styles.strong}>{title}</Text>
        <Text variant="caption" muted>
          {children}
        </Text>
      </View>
    </Pressable>
  );
}

const styles = StyleSheet.create((theme) => ({
  screen: { flex: 1, backgroundColor: theme.colors.background },
  // Room below the last row for the undo bar.
  content: { padding: theme.spacing.lg, paddingBottom: 96 },
  notice: { textAlign: 'center', padding: theme.spacing.lg },
  card: {
    backgroundColor: theme.colors.surfaceVariant,
    borderRadius: theme.radius.md,
    padding: theme.spacing.md,
    gap: theme.spacing.sm,
  },
  row: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.md },
  grow: { flex: 1 },
  strong: { fontWeight: '600' },
  // Spaced within the member, so a member folding away takes its spacing with it.
  member: { paddingTop: theme.spacing.sm },
  memberName: { flexDirection: 'row', alignItems: 'center', minHeight: 20 },
  volume: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.sm },
  // Android's slider insets its track by the thumb's radius; this lines the
  // track up with the names above it.
  slider: { flex: 1, marginHorizontal: Platform.OS === 'android' ? -14 : 0 },
  level: { width: 28, textAlign: 'right' },
  end: { width: 32, height: 32 },
  remove: {
    borderRadius: 16,
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor: theme.colors.background,
  },
  section: { marginTop: theme.spacing.lg, letterSpacing: 0.8, textTransform: 'uppercase' },
  other: { paddingVertical: theme.spacing.sm },
  icon: {
    width: 32,
    height: 32,
    borderRadius: theme.radius.sm,
    backgroundColor: theme.colors.surfaceVariant,
    alignItems: 'center',
    justifyContent: 'center',
  },
  unavailable: { opacity: 0.45 },
  // Joining or leaving the group, until the server confirms it.
  unconfirmed: { opacity: 0.6 },
  // Inset as the active group's card is, so an open row's card has room around what it holds.
  otherCard: { marginBottom: theme.spacing.xs, paddingHorizontal: theme.spacing.md },
  otherCardBackground: {
    ...StyleSheet.absoluteFillObject,
    backgroundColor: theme.colors.surfaceVariant,
    borderRadius: theme.radius.md,
  },
  actions: { paddingBottom: theme.spacing.sm },
  // In line with the row's icon and name above.
  action: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    gap: theme.spacing.md,
    paddingVertical: theme.spacing.sm,
  },
  actionIcon: { backgroundColor: theme.colors.background },
  warning: { color: theme.colors.danger },
  pressed: { opacity: 0.6 },
  toast: {
    position: 'absolute',
    left: theme.spacing.lg,
    right: theme.spacing.lg,
    bottom: theme.spacing.xl,
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    backgroundColor: theme.colors.text,
    borderRadius: theme.radius.md,
    paddingHorizontal: theme.spacing.lg,
    paddingVertical: theme.spacing.md,
  },
  toastText: { flex: 1, color: theme.colors.background },
  toastAction: { color: theme.colors.background, fontWeight: '700' },
}));
