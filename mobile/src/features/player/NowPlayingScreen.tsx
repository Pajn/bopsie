import type { RepeatMode } from '@bopsie/client/api/schemas/player';
import Slider from '@react-native-community/slider';
import { Stack, useRouter } from 'expo-router';
import { useState } from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { Artwork } from '@/features/library/Artwork';
import { useOutputs } from '@/features/outputs/useOutputs';
import { formatDuration, queuePosition } from '@/player/progress';
import { Icon, type IconName } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { itemLinks, nowPlaying } from './nowPlaying';
import { nextRepeatMode } from './repeat';
import { useNow } from './useNow';
import { usePlaybackActions, useQueue } from './usePlayback';

export function NowPlayingScreen() {
  const queue = useQueue().data;
  const playback = usePlaybackActions();
  const { theme } = useUnistyles();
  const playing = queue?.state === 'playing';
  const now = useNow(playing);
  const [dragging, setDragging] = useState<number | null>(null);
  const current = queue ? nowPlaying(queue) : null;
  const router = useRouter();
  const active = useOutputs()?.active;

  if (!queue || !current) {
    return (
      <View style={styles.screen}>
        <Text muted style={styles.center}>
          Nothing is playing.
        </Text>
      </View>
    );
  }

  const position = dragging ?? queuePosition(queue, now);
  const duration = current.duration;
  const hasLinks = queue.current_item ? itemLinks(queue.current_item).length > 0 : false;

  return (
    <View style={styles.screen}>
      <Stack.Screen
        options={{
          headerRight: hasLinks
            ? () => (
                <Pressable
                  onPress={() => router.push('/player-menu')}
                  hitSlop={12}
                  accessibilityRole="button"
                  accessibilityLabel="More"
                  testID="player-more"
                >
                  <Icon name="more-horizontal" size={24} />
                </Pressable>
              )
            : () => null,
        }}
      />
      <View style={styles.artwork}>
        <Artwork item={current.artwork} size={300} imageSize={1024} />
      </View>
      <View style={styles.names}>
        <Text variant="heading" numberOfLines={2} style={styles.center} testID="player-title">
          {current.title}
        </Text>
        <Text muted numberOfLines={2} style={styles.center}>
          {current.subtitle}
        </Text>
        <View style={styles.pills}>
          <Pressable
            onPress={() => router.push('/outputs')}
            style={({ pressed }) => [styles.pill, styles.where, pressed && styles.pressed]}
            accessibilityRole="button"
            accessibilityLabel={`Playing on ${active?.name ?? 'this phone'}. Choose where to play`}
            testID="player-outputs"
          >
            <Icon name="speaker" size={14} color={theme.colors.accent} />
            <Text variant="label" style={styles.pillText} numberOfLines={1}>
              {active?.name ?? 'Choose a player'}
            </Text>
            <Icon name="chevron-down" size={14} color={theme.colors.accent} />
          </Pressable>
          <Pressable
            onPress={() => router.push('/queue')}
            style={({ pressed }) => [styles.pill, pressed && styles.pressed]}
            accessibilityRole="button"
            accessibilityLabel="Queue"
            testID="player-queue"
          >
            <Icon name="list" size={14} color={theme.colors.accent} />
            <Text variant="label" style={styles.pillText}>
              Queue
            </Text>
          </Pressable>
        </View>
      </View>
      <View>
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
          minimumTrackTintColor={theme.colors.accent}
          maximumTrackTintColor={theme.colors.border}
          thumbTintColor={theme.colors.accent}
          testID="player-seek"
        />
        <View style={styles.times}>
          <Text variant="caption" muted testID="player-position">
            {formatDuration(position)}
          </Text>
          <Text variant="caption" muted>
            {duration ? formatDuration(duration) : ''}
          </Text>
        </View>
      </View>
      <View style={styles.controls}>
        <Toggle
          icon="shuffle"
          label="Shuffle"
          on={queue.shuffle_enabled}
          hidden={queue.source !== null}
          onPress={() => playback.shuffle(!queue.shuffle_enabled)}
          testID="player-shuffle"
        />
        <Control icon="skip-back" label="Previous" onPress={() => playback.command('previous')} />
        <Control
          icon={playing ? 'pause' : 'play'}
          label={playing ? 'Pause' : 'Play'}
          onPress={() => playback.command('play_pause')}
          large
          testID="player-toggle"
        />
        <Control
          icon="skip-forward"
          label="Next"
          onPress={() => playback.command('next')}
          testID="player-next"
        />
        <Toggle
          icon="repeat"
          label={REPEAT_LABELS[queue.repeat_mode]}
          on={queue.repeat_mode !== 'off'}
          badge={queue.repeat_mode === 'one' ? '1' : undefined}
          hidden={queue.source !== null}
          onPress={() => playback.repeat(nextRepeatMode(queue.repeat_mode))}
          testID="player-repeat"
        />
      </View>
    </View>
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
      <Icon name={icon} size={22} color={on ? theme.colors.accent : theme.colors.textMuted} />
      {badge ? (
        <Text variant="caption" style={styles.badge}>
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
  testID,
}: {
  icon: IconName;
  label: string;
  onPress: () => void;
  large?: boolean;
  testID?: string;
}) {
  return (
    <Pressable
      onPress={onPress}
      hitSlop={12}
      accessibilityRole="button"
      accessibilityLabel={label}
      style={({ pressed }) => [large && styles.large, pressed && styles.pressed]}
      {...(testID && { testID })}
    >
      <Icon name={icon} size={large ? 36 : 28} />
    </Pressable>
  );
}

const styles = StyleSheet.create((theme) => ({
  screen: {
    flex: 1,
    backgroundColor: theme.colors.background,
    padding: theme.spacing.xl,
    justifyContent: 'center',
    gap: theme.spacing.xl,
  },
  artwork: { alignItems: 'center' },
  names: { gap: theme.spacing.xs },
  center: { textAlign: 'center' },
  times: { flexDirection: 'row', justifyContent: 'space-between' },
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
  pills: {
    flexDirection: 'row',
    justifyContent: 'center',
    gap: theme.spacing.sm,
    marginTop: theme.spacing.sm,
  },
  pill: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.xs,
    paddingHorizontal: theme.spacing.md,
    paddingVertical: theme.spacing.xs,
    borderRadius: theme.radius.full,
    backgroundColor: theme.colors.surfaceVariant,
  },
  // The player's name gives way first when the two do not fit.
  where: { flexShrink: 1 },
  pillText: { color: theme.colors.accent, flexShrink: 1 },
  pressed: { opacity: 0.6 },
}));
