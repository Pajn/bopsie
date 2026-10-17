import type { RepeatMode } from '@bopsie/client/api/schemas/player';
import Slider from '@react-native-community/slider';
import { useRouter } from 'expo-router';
import { useCallback, useLayoutEffect, useRef, useState } from 'react';
import { Pressable, View, type ViewInstance } from 'react-native';
import { Gesture, GestureDetector, type GestureType } from 'react-native-gesture-handler';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import {
  chapterName,
  playsABook,
  sleeping,
  sleepLeft,
  speedLabel,
} from '@/features/audiobooks/model';
import { useActiveGroup } from '@/features/outputs/ActiveGroupProvider';
import { useOutputs } from '@/features/outputs/useOutputs';
import { formatDuration, queuePosition } from '@/player/progress';
import { haptics } from '@/ui/haptics';
import { Icon, type IconName } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { FULL_COVER, useCoverSlot } from './FlyingCover';
import type { Frame } from './MorphPanel';
import { emptyQueue, itemLinks, NOTHING_PLAYING, nowPlaying } from './nowPlaying';
import { OutputsPanel, PILL_CHEVRON, PillContent } from './OutputsPanel';
import { QUEUE_BAR_HEIGHT, QueuePanel } from './QueuePanel';
import { nextRepeatMode } from './repeat';
import { collapsePlayer, coverSwipe } from './sheetStore';
import { useNow } from './useNow';
import { usePlaybackActions, useQueue } from './usePlayback';
import { usePlayerColors } from './usePlayerTint';
import { useSheetDrag } from './useSheetDrag';
import { useSkipSwipe } from './useSkipSwipe';

/**
 * The full player: what plays, where, and its controls, with the queue and Play on growing out
 * of it. It is the open face of the player sheet; `open` says whether it is showing, and
 * `travel` is how far the sheet moves, for its header and artwork to drag it down by.
 */
export function PlayerBody({ open, travel }: { open: boolean; travel: number }) {
  const { activeId } = useActiveGroup();
  const loaded = useQueue().data;
  // With nothing loaded the player keeps its layout, empty, so where to play stays at hand.
  const queue = loaded ?? emptyQueue(activeId ?? 'none');
  const playback = usePlaybackActions();
  const colors = usePlayerColors();
  const { theme } = useUnistyles();
  const playing = queue?.state === 'playing';
  // The position only moves while it can be seen.
  const now = useNow(playing && open);
  const grip = useSheetDrag(travel, 'down');
  const artworkGrip = useSheetDrag(travel, 'down');
  const cover = useCoverSlot('full');
  const skip = useCallback((to: 'next' | 'previous') => playback.command(to), [playback]);
  const artworkSwipe = useSkipSwipe(coverSwipe, FULL_COVER, skip);
  const [dragging, setDragging] = useState<number | null>(null);
  const idle = nowPlaying(queue) === null;
  const current = nowPlaying(queue) ?? NOTHING_PLAYING;
  const router = useRouter();
  const active = useOutputs()?.active;
  const [size, setSize] = useState<{ width: number; height: number } | null>(null);
  const [bar, setBar] = useState<Frame | null>(null);
  const [pill, setPill] = useState<Frame | null>(null);
  const screen = useRef<ViewInstance>(null);
  const pillSpace = useRef<ViewInstance>(null);

  // Where the pill sits in the player, which anything above it moves, so after every render.
  useLayoutEffect(() => {
    const root = screen.current;
    if (!root || !pillSpace.current) return;
    pillSpace.current.measureLayout(root, (x, y, width, height) =>
      setPill((last) =>
        last && last.x === x && last.y === y && last.width === width && last.height === height
          ? last
          : { x, y, width, height },
      ),
    );
  });

  const position = dragging ?? queuePosition(queue, now);
  const duration = current.duration;
  const hasLinks = queue.current_item ? itemLinks(queue.current_item).length > 0 : false;
  const where = active?.name ?? 'Choose a player';
  // A book has a speed and a sleep timer where music has shuffle and repeat.
  const book = playsABook(queue);
  const chapter = book ? chapterName(queue) : null;
  const asleep = book ? sleepLeft(queue, now) : null;

  return (
    <View
      ref={screen}
      style={styles.screen}
      onLayout={({ nativeEvent }) => setSize(nativeEvent.layout)}
    >
      <Header grip={grip} {...(hasLinks && { onMore: () => router.push('/player-menu') })} />
      <View style={styles.main}>
        <GestureDetector gesture={idle ? artworkGrip : Gesture.Race(artworkGrip, artworkSwipe)}>
          <View style={styles.artwork}>
            {/* The artwork is drawn over this, by the sheet, having flown in from the mini player. */}
            <View ref={cover.ref} onLayout={cover.onLayout} style={styles.cover} />
          </View>
        </GestureDetector>
        <View style={styles.names}>
          <Text variant="heading" numberOfLines={2} style={styles.center} testID="player-title">
            {current.title}
          </Text>
          <Text muted numberOfLines={2} style={styles.center}>
            {current.subtitle}
          </Text>
          <View style={styles.where}>
            {/* The pill is drawn over this stand-in, from which it opens Play on over the player. */}
            <View
              ref={pillSpace}
              style={styles.pill}
              importantForAccessibility="no-hide-descendants"
              accessibilityElementsHidden
            >
              <PillContent name={where} />
              <View style={styles.pillChevron} />
            </View>
          </View>
        </View>
        <View style={idle && styles.disabled} pointerEvents={idle ? 'none' : 'auto'}>
          <Slider
            minimumValue={0}
            maximumValue={duration ?? 1}
            value={duration ? position : 0}
            disabled={!duration}
            onSlidingStart={(value) => setDragging(value)}
            onValueChange={(value) => dragging !== null && setDragging(value)}
            onSlidingComplete={(value) => {
              setDragging(null);
              playback.seek(value);
            }}
            minimumTrackTintColor={colors.accent}
            maximumTrackTintColor={colors.surface}
            thumbTintColor={colors.accent}
            testID="player-seek"
          />
          <View style={styles.times}>
            <Text variant="caption" muted testID="player-position">
              {idle ? '' : queue?.buffering ? 'Starting…' : formatDuration(position)}
            </Text>
            {chapter || asleep ? (
              <Text
                variant="caption"
                muted
                numberOfLines={1}
                style={styles.chapter}
                testID="player-chapter"
              >
                {[chapter, asleep && `Sleep: ${asleep}`].filter(Boolean).join(' · ')}
              </Text>
            ) : null}
            <Text variant="caption" muted>
              {duration ? formatDuration(duration) : ''}
            </Text>
          </View>
        </View>
        <View style={styles.controls}>
          {book ? (
            <Pressable
              onPress={() => router.push('/player-speed')}
              hitSlop={12}
              accessibilityRole="button"
              accessibilityLabel={`Playback speed ${speedLabel(queue.playback_speed)}`}
              style={({ pressed }) => [styles.toggle, pressed && styles.pressed]}
              testID="player-speed"
            >
              <Text variant="label" style={queue.playback_speed !== 1 && { color: colors.accent }}>
                {speedLabel(queue.playback_speed)}
              </Text>
            </Pressable>
          ) : (
            <Toggle
              icon="shuffle"
              label="Shuffle"
              on={queue.shuffle_enabled}
              hidden={queue.source !== null || idle}
              onPress={() => {
                haptics.toggle(!queue.shuffle_enabled);
                playback.shuffle(!queue.shuffle_enabled);
              }}
              testID="player-shuffle"
            />
          )}
          <Control
            icon="skip-back"
            label="Previous"
            onPress={() => playback.command('previous')}
            disabled={idle}
          />
          <Control
            icon={playing ? 'pause' : 'play'}
            label={playing ? 'Pause' : 'Play'}
            onPress={() => playback.command('play_pause')}
            large
            disabled={idle}
            testID="player-toggle"
          />
          <Control
            icon="skip-forward"
            label="Next"
            onPress={() => playback.command('next')}
            disabled={idle}
            testID="player-next"
          />
          {book ? (
            <Pressable
              onPress={() => router.push('/player-sleep')}
              hitSlop={12}
              accessibilityRole="button"
              accessibilityLabel={asleep ? `Sleep timer: ${asleep}` : 'Sleep timer'}
              style={({ pressed }) => [styles.toggle, pressed && styles.pressed]}
              testID="player-sleep"
            >
              <Icon
                name="moon"
                size={22}
                color={sleeping(queue) ? colors.accent : theme.colors.textMuted}
              />
            </Pressable>
          ) : (
            <Toggle
              icon="repeat"
              label={REPEAT_LABELS[queue.repeat_mode]}
              on={queue.repeat_mode !== 'off'}
              badge={queue.repeat_mode === 'one' ? '1' : undefined}
              hidden={queue.source !== null || idle}
              onPress={() => {
                const mode = nextRepeatMode(queue.repeat_mode);
                haptics.toggle(mode !== 'off');
                playback.repeat(mode);
              }}
              testID="player-repeat"
            />
          )}
        </View>
      </View>
      {/* The queue's bar is drawn over this space, from which it opens over the whole player. */}
      <View style={styles.barSpace} onLayout={({ nativeEvent }) => setBar(nativeEvent.layout)} />
      {bar && size ? <QueuePanel queue={queue} bar={bar} player={size} ready={open} /> : null}
      {pill && size ? <OutputsPanel name={where} pill={pill} player={size} ready={open} /> : null}
    </View>
  );
}

/** Closes the player, and opens its menu where there is one; it drags the player down too. */
function Header({ grip, onMore }: { grip: GestureType; onMore?: () => void }) {
  return (
    <GestureDetector gesture={grip}>
      <View style={styles.header}>
        <Pressable
          onPress={collapsePlayer}
          hitSlop={12}
          accessibilityRole="button"
          accessibilityLabel="Close the player"
          testID="player-close"
        >
          <Icon name="chevron-down" size={24} />
        </Pressable>
        <Text variant="heading" style={styles.headerTitle} accessibilityRole="header">
          Now playing
        </Text>
        {onMore ? (
          <Pressable
            onPress={onMore}
            hitSlop={12}
            accessibilityRole="button"
            accessibilityLabel="More"
            testID="player-more"
          >
            <Icon name="more-horizontal" size={24} />
          </Pressable>
        ) : null}
      </View>
    </GestureDetector>
  );
}

const REPEAT_LABELS: Record<RepeatMode, string> = {
  off: 'Repeat: off',
  all: 'Repeat: all',
  one: 'Repeat: this track',
};

/**
 * Shuffle or repeat: accented while on, with a badge for repeating one track.
 * Hidden, though still holding its place, while a source plays, which keeps its own.
 */
function Toggle({
  icon,
  label,
  on,
  badge,
  hidden,
  onPress,
  testID,
}: {
  icon: IconName;
  label: string;
  on: boolean;
  badge?: string | undefined;
  hidden: boolean;
  onPress: () => void;
  testID: string;
}) {
  const { theme } = useUnistyles();
  const colors = usePlayerColors();
  if (hidden) return <View style={styles.toggle} />;
  return (
    <Pressable
      onPress={onPress}
      hitSlop={12}
      accessibilityRole="switch"
      accessibilityLabel={label}
      accessibilityState={{ checked: on }}
      style={({ pressed }) => [styles.toggle, pressed && styles.pressed]}
      testID={testID}
    >
      <Icon name={icon} size={22} color={on ? colors.accent : theme.colors.textMuted} />
      {badge ? (
        <Text variant="caption" style={[styles.badge, { color: colors.accent }]}>
          {badge}
        </Text>
      ) : null}
    </Pressable>
  );
}

function Control({
  icon,
  label,
  onPress,
  large,
  disabled,
  testID,
}: {
  icon: IconName;
  label: string;
  onPress: () => void;
  large?: boolean;
  /** Shown faded and taking no presses, as with nothing to play. */
  disabled?: boolean;
  testID?: string;
}) {
  const size = large ? 36 : 28;
  const colors = usePlayerColors();
  return (
    <Pressable
      onPress={onPress}
      disabled={disabled}
      hitSlop={12}
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ disabled: disabled ?? false }}
      style={({ pressed }) => [
        large && styles.large,
        large && { backgroundColor: colors.surface },
        pressed && styles.pressed,
        disabled && styles.disabled,
      ]}
      {...(testID && { testID })}
    >
      <Icon
        name={icon}
        size={size}
        // A triangle's weight sits left of its box, so in a round button it looks off
        // centre; nudged right by where its weight falls short, it looks centred.
        {...(icon === 'play' && { style: { marginLeft: Math.round(size * 0.08) } })}
      />
    </Pressable>
  );
}

const styles = StyleSheet.create((theme, rt) => ({
  screen: {
    flex: 1,
    paddingHorizontal: theme.spacing.xl,
    paddingBottom: theme.spacing.lg + rt.insets.bottom,
  },
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.xxl,
    height: 56,
    marginTop: rt.insets.top,
    marginHorizontal: theme.spacing.lg - theme.spacing.xl,
  },
  headerTitle: { flex: 1 },
  main: { flex: 1, justifyContent: 'center', gap: theme.spacing.xl },
  barSpace: { height: QUEUE_BAR_HEIGHT },
  artwork: { alignItems: 'center' },
  cover: { width: FULL_COVER, height: FULL_COVER },
  names: { gap: theme.spacing.xs },
  center: { textAlign: 'center' },
  times: { flexDirection: 'row', justifyContent: 'space-between', gap: theme.spacing.sm },
  chapter: { flex: 1, textAlign: 'center' },
  controls: { flexDirection: 'row', justifyContent: 'space-between', alignItems: 'center' },
  toggle: { width: 32, height: 32, alignItems: 'center', justifyContent: 'center' },
  badge: {
    position: 'absolute',
    top: -6,
    right: -4,
    fontSize: 10,
    lineHeight: 12,
    fontWeight: '700',
    color: theme.colors.accent,
  },
  large: {
    width: 72,
    height: 72,
    borderRadius: 36,
    backgroundColor: theme.colors.surfaceVariant,
    alignItems: 'center',
    justifyContent: 'center',
  },
  where: { alignItems: 'center', marginTop: theme.spacing.sm },
  // Laid out as the pill is, unseen.
  pill: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.xs,
    paddingHorizontal: theme.spacing.md,
    paddingVertical: theme.spacing.xs,
    opacity: 0,
  },
  pillChevron: { width: PILL_CHEVRON, height: PILL_CHEVRON },
  pressed: { opacity: 0.6 },
  disabled: { opacity: 0.35 },
}));
