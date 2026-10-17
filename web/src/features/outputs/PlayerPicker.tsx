import {
  type ActiveGroup,
  describeOutputs,
  type OtherOutput,
} from '@bopsie/client/outputs/outputs';
import { type ReactNode, useCallback, useEffect, useState } from 'react';
import { Modal, Pressable, ScrollView, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { describeError } from '@/lib/errors';
import { Artwork } from '@/ui/Artwork';
import { IconButton, VolumeSlider } from '@/ui/controls';
import { Icon, type IconName } from '@/ui/Icon';
import { InlineError, Text, type WebPress } from '@/ui/primitives';
import { useOutputActions } from './useOutputActions';
import { usePredictedPlayers } from './usePredictedPlayers';

const UNDO_MS = 6000;

/**
 * The player the web controls, as a button that opens the players sheet: as the app's output
 * sheet, the active group on top with its volumes and members, and every other player or group
 * below, each opening to what it can do: join the group, take over its queue, or become the one
 * the web controls. On a phone the button is an icon and the sheet rises from the foot.
 */
export function PlayerPicker({
  activeId,
  choose,
  title,
  artwork,
  canMove,
}: {
  /** The player the web controls. */
  activeId: string;
  choose: (playerId: string) => void;
  /** What plays on it, if anything. */
  title: string | null;
  artwork: string | null;
  /** Whether it has a queue to move elsewhere. */
  canMove: boolean;
}) {
  const { theme } = useUnistyles();
  const { players, unconfirmed, predict, forget } = usePredictedPlayers();
  const [shown, setShown] = useState(false);
  const [open, setOpen] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [undo, setUndo] = useState<{ name: string; run: () => void } | null>(null);
  const report = useCallback((e: unknown) => setError(describeError(e)), []);
  const actions = useOutputActions({ players, activeId, choose, predict, forget, report });
  const outputs = players ? describeOutputs(players, activeId, null) : null;
  const name = outputs?.active?.name ?? players?.find((p) => p.player_id === activeId)?.name;

  useEffect(() => {
    if (!undo) return;
    const timer = setTimeout(() => setUndo(null), UNDO_MS);
    return () => clearTimeout(timer);
  }, [undo]);

  const close = () => {
    setShown(false);
    setOpen(null);
    setError(null);
  };

  return (
    <>
      <Pressable
        onPress={() => setShown(true)}
        accessibilityRole="button"
        accessibilityLabel={`Players: ${name ?? 'none'}`}
        {...({ title: 'Players' } as object)}
        style={({ pressed, hovered }: WebPress) => [
          styles.trigger,
          hovered && styles.triggerHover,
          pressed && styles.pressed,
        ]}
        testID="players-open"
      >
        <Icon name={outputs?.active && outputs.active.members.length > 1 ? 'layers' : 'speaker'} />
        <Text numberOfLines={1} style={[styles.strong, styles.wideOnly, styles.triggerName]}>
          {name ?? 'Players'}
        </Text>
        <View style={styles.wideOnly}>
          <Icon name="chevron-up" size={14} color={theme.colors.textMuted} />
        </View>
      </Pressable>
      <Modal visible={shown} transparent animationType="fade" onRequestClose={close}>
        <Pressable
          style={styles.backdrop}
          onPress={close}
          accessibilityLabel="Close players"
          {...({ tabIndex: -1 } as object)}
        />
        <View style={styles.sheet} role="dialog" aria-label="Players">
          <View style={styles.header}>
            <Text variant="heading" style={styles.grow}>
              Players
            </Text>
            <IconButton icon="x" label="Close players" variant="ghost" size="sm" onPress={close} />
          </View>
          <ScrollView contentContainerStyle={styles.content}>
            <InlineError message={error} />
            {!outputs ? (
              <Text muted style={styles.notice}>
                Looking for players…
              </Text>
            ) : null}
            {outputs?.active ? (
              <ActiveCard
                active={outputs.active}
                title={title}
                artwork={artwork}
                unconfirmed={unconfirmed}
                actions={actions}
              />
            ) : null}
            {outputs && outputs.others.length > 0 ? (
              <Text variant="label" muted style={styles.section}>
                Other players
              </Text>
            ) : null}
            {outputs?.others.map((other) => (
              <OtherRow
                key={other.id}
                other={other}
                active={outputs.active}
                open={open === other.id}
                onToggle={() => setOpen(open === other.id ? null : other.id)}
                unconfirmed={unconfirmed.has(other.id)}
                canMove={canMove}
                actions={actions}
                onMoved={(run) => {
                  setOpen(null);
                  setUndo({ name: other.name, run });
                }}
                onChosen={() => setOpen(null)}
              />
            ))}
          </ScrollView>
          {undo ? (
            <View style={styles.undo} role="status">
              <Text style={styles.undoText} numberOfLines={1}>
                Moved to {undo.name}
              </Text>
              <Pressable
                onPress={() => {
                  undo.run();
                  setUndo(null);
                }}
                accessibilityRole="button"
              >
                <Text style={styles.undoAction}>Undo</Text>
              </Pressable>
            </View>
          ) : null}
        </View>
      </Modal>
    </>
  );
}

type Actions = ReturnType<typeof useOutputActions>;

function ActiveCard({
  active,
  title,
  artwork,
  unconfirmed,
  actions,
}: {
  active: ActiveGroup;
  title: string | null;
  artwork: string | null;
  unconfirmed: ReadonlySet<string>;
  actions: Actions;
}) {
  const grouped = active.members.length > 1;
  return (
    <View style={styles.card}>
      <View style={styles.row}>
        <Artwork uri={artwork} size={44} placeholder="speaker" />
        <View style={styles.grow}>
          <Text style={styles.strong} numberOfLines={1}>
            {active.name}
          </Text>
          <Text variant="caption" muted numberOfLines={1}>
            {!active.available ? 'Unavailable' : (title ?? 'Nothing playing')}
          </Text>
        </View>
      </View>
      {active.volume !== null ? (
        <View style={styles.volume}>
          <View style={styles.grow}>
            <VolumeSlider
              value={active.volume}
              label={grouped ? `the group ${active.name}` : active.name}
              onChange={actions.setGroupVolume}
              {...(active.muted !== null && {
                muted: active.muted,
                onMute: (muted: boolean) => actions.setMuted(active.id, muted),
              })}
            />
          </View>
          {/* Lines the group's slider up with the members'. */}
          {grouped ? <View style={styles.end} /> : null}
        </View>
      ) : null}
      {grouped
        ? active.members.map((member) => {
            const remove = member.removable ? (
              <IconButton
                icon="x"
                label={`Remove ${member.name} from the group`}
                variant="ghost"
                size="sm"
                onPress={() => actions.remove(member, active.members)}
              />
            ) : null;
            return (
              <View
                key={member.id}
                style={[styles.member, unconfirmed.has(member.id) && styles.unconfirmed]}
              >
                <View style={styles.memberName}>
                  <Text variant="label" style={styles.grow} numberOfLines={1}>
                    {member.name}
                  </Text>
                  {member.volume === null ? <View style={styles.end}>{remove}</View> : null}
                </View>
                {member.volume !== null ? (
                  <View style={styles.volume}>
                    <View style={styles.grow}>
                      <VolumeSlider
                        value={member.volume}
                        label={member.name}
                        onChange={(level) => actions.setVolume(member.id, level)}
                        {...(member.muted !== null && {
                          muted: member.muted,
                          onMute: (muted: boolean) => actions.setMuted(member.id, muted),
                        })}
                      />
                    </View>
                    <View style={styles.end}>{remove}</View>
                  </View>
                ) : null}
              </View>
            );
          })
        : null}
    </View>
  );
}

function OtherRow({
  other,
  active,
  open,
  onToggle,
  unconfirmed,
  canMove,
  actions,
  onMoved,
  onChosen,
}: {
  other: OtherOutput;
  active: ActiveGroup | null;
  open: boolean;
  onToggle: () => void;
  unconfirmed: boolean;
  canMove: boolean;
  actions: Actions;
  onMoved: (undo: () => void) => void;
  onChosen: () => void;
}) {
  const { theme } = useUnistyles();
  const icon: IconName = other.isGroup ? 'layers' : 'speaker';
  const what = other.isGroup ? 'group' : 'player';
  return (
    <View style={[styles.otherCard, open && styles.otherOpen, unconfirmed && styles.unconfirmed]}>
      <Pressable
        onPress={onToggle}
        disabled={!other.available}
        style={({ pressed, hovered }: WebPress) => [
          styles.row,
          styles.other,
          hovered && other.available && !open && styles.otherHover,
          pressed && styles.pressed,
        ]}
        accessibilityRole="button"
        accessibilityState={{ expanded: open, disabled: !other.available }}
        testID={`output-${other.id}`}
      >
        <View style={[styles.icon, open && styles.iconOpen, !other.available && styles.faded]}>
          <Icon name={icon} size={16} />
        </View>
        <View style={[styles.grow, !other.available && styles.faded]}>
          <Text style={open ? styles.strong : undefined} numberOfLines={1}>
            {other.name}
          </Text>
          <Text variant="caption" muted numberOfLines={1}>
            {other.status}
          </Text>
        </View>
        {other.available ? (
          <Icon name={open ? 'chevron-up' : 'chevron-down'} size={18} color={theme.colors.text} />
        ) : null}
      </Pressable>
      {open ? (
        <View style={styles.actions}>
          {active && other.canJoin ? (
            <Action
              icon="plus"
              title="Join this group"
              onPress={() => {
                actions.join(other.id);
                onChosen();
              }}
            >
              {`${other.name} plays along with ${active.name}.`}
              {other.playing ? (
                <Text variant="caption" style={styles.warning}>
                  {` ${other.playing} stops.`}
                </Text>
              ) : null}
            </Action>
          ) : null}
          {active && canMove ? (
            <Action
              icon="arrow-right"
              title="Move playback here"
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
            onPress={() => {
              actions.control(other.id);
              onChosen();
            }}
          >
            Play, pause and change volume here. Nothing else stops.
          </Action>
        </View>
      ) : null}
    </View>
  );
}

function Action({
  icon,
  title,
  children,
  onPress,
}: {
  icon: IconName;
  title: string;
  children: ReactNode;
  onPress: () => void;
}) {
  return (
    <Pressable
      onPress={onPress}
      style={({ pressed, hovered }: WebPress) => [
        styles.action,
        hovered && styles.actionHover,
        pressed && styles.pressed,
      ]}
      accessibilityRole="button"
      accessibilityLabel={title}
    >
      <View style={[styles.icon, styles.iconOpen]}>
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
  trigger: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.sm,
    height: { xs: 44, md: 34 },
    minWidth: { xs: 44, md: 0 },
    maxWidth: 200,
    justifyContent: 'center',
    paddingHorizontal: { xs: 0, md: theme.spacing.md },
    borderRadius: theme.radius.md,
    backgroundColor: { xs: 'transparent', md: theme.colors.surfaceVariant },
  },
  triggerHover: { backgroundColor: theme.colors.border },
  triggerName: { flexShrink: 1 },
  wideOnly: { display: { xs: 'none', md: 'flex' } },
  pressed: { opacity: 0.6 },
  backdrop: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    backgroundColor: 'rgba(0, 0, 0, 0.3)',
  },
  // Above the now-playing bar at its end where there is room; risen from the foot on a phone.
  sheet: {
    position: 'absolute',
    right: { xs: 0, md: theme.spacing.xl },
    left: { xs: 0, md: 'auto' },
    bottom: { xs: 0, md: 76 },
    width: { xs: 'auto', md: 400 },
    maxHeight: { xs: '85%', md: '75%' },
    backgroundColor: theme.colors.surface,
    borderRadius: theme.radius.lg,
    borderBottomLeftRadius: { xs: 0, md: theme.radius.lg },
    borderBottomRightRadius: { xs: 0, md: theme.radius.lg },
    borderWidth: 1,
    borderColor: theme.colors.border,
    overflow: 'hidden',
    boxShadow: '0 8px 32px rgba(0, 0, 0, 0.25)',
  },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingLeft: theme.spacing.lg,
    paddingRight: theme.spacing.sm,
    paddingTop: theme.spacing.md,
  },
  content: { padding: theme.spacing.lg, paddingTop: theme.spacing.sm },
  notice: { textAlign: 'center', padding: theme.spacing.lg },
  card: {
    backgroundColor: theme.colors.surfaceVariant,
    borderRadius: theme.radius.md,
    padding: theme.spacing.md,
    gap: theme.spacing.sm,
  },
  row: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.md },
  grow: { flex: 1, minWidth: 0 },
  strong: { fontWeight: '600' },
  volume: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.xs },
  // Room for a member's remove button, which the group's slider keeps too, to line up.
  end: { width: 28, alignItems: 'center' },
  member: { paddingTop: theme.spacing.xs },
  memberName: { flexDirection: 'row', alignItems: 'center', minHeight: 20 },
  section: {
    marginTop: theme.spacing.lg,
    marginBottom: theme.spacing.xs,
    letterSpacing: 0.8,
    textTransform: 'uppercase',
  },
  // Inset as the active group's card is, so an open row's card has room around what it holds.
  otherCard: {
    marginBottom: theme.spacing.xs,
    paddingHorizontal: theme.spacing.md,
    borderRadius: theme.radius.md,
  },
  otherOpen: { backgroundColor: theme.colors.surfaceVariant },
  other: {
    paddingVertical: theme.spacing.sm,
    marginHorizontal: -theme.spacing.md,
    paddingHorizontal: theme.spacing.md,
    borderRadius: theme.radius.md,
  },
  otherHover: { backgroundColor: theme.colors.surfaceVariant },
  icon: {
    width: 32,
    height: 32,
    borderRadius: theme.radius.sm,
    backgroundColor: theme.colors.surfaceVariant,
    alignItems: 'center',
    justifyContent: 'center',
  },
  // On the open card, the icon's tile turns light, as the actions' are.
  iconOpen: { backgroundColor: theme.colors.surface },
  faded: { opacity: 0.45 },
  // Joining or leaving the group, until the server confirms it.
  unconfirmed: { opacity: 0.6 },
  actions: { paddingBottom: theme.spacing.sm },
  // In line with the row's icon and name above.
  action: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    gap: theme.spacing.md,
    paddingVertical: theme.spacing.sm,
    marginHorizontal: -theme.spacing.sm,
    paddingHorizontal: theme.spacing.sm,
    borderRadius: theme.radius.md,
  },
  actionHover: { backgroundColor: theme.colors.border },
  warning: { color: theme.colors.danger },
  undo: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    margin: theme.spacing.md,
    marginTop: 0,
    backgroundColor: theme.colors.text,
    borderRadius: theme.radius.md,
    paddingHorizontal: theme.spacing.lg,
    paddingVertical: theme.spacing.md,
  },
  undoText: { flex: 1, color: theme.colors.background },
  undoAction: { color: theme.colors.background, fontWeight: '700' },
}));
