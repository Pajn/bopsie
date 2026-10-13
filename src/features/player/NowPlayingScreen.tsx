import Slider from '@react-native-community/slider';
import { useRouter } from 'expo-router';
import { useState } from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { Artwork } from '@/features/library/Artwork';
import { useOutputs } from '@/features/outputs/useOutputs';
import { formatDuration, queuePosition } from '@/player/progress';
import { Icon, type IconName } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { nowPlaying } from './nowPlaying';
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

  return (
    <View style={styles.screen}>
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
        <Pressable
          onPress={() => router.push('/outputs')}
          style={({ pressed }) => [styles.where, pressed && styles.pressed]}
          accessibilityRole="button"
          accessibilityLabel={`Playing on ${active?.name ?? 'this phone'}. Choose where to play`}
          testID="player-outputs"
        >
          <Icon name="speaker" size={14} color={theme.colors.accent} />
          <Text variant="label" style={styles.whereText} numberOfLines={1}>
            {active?.name ?? 'Choose a player'}
          </Text>
          <Icon name="chevron-down" size={14} color={theme.colors.accent} />
        </Pressable>
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
      </View>
    </View>
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
  controls: { flexDirection: 'row', justifyContent: 'space-evenly', alignItems: 'center' },
  large: {
    width: 72,
    height: 72,
    borderRadius: 36,
    backgroundColor: theme.colors.surfaceVariant,
    alignItems: 'center',
    justifyContent: 'center',
  },
  where: {
    flexDirection: 'row',
    alignItems: 'center',
    alignSelf: 'center',
    gap: theme.spacing.xs,
    marginTop: theme.spacing.sm,
    paddingHorizontal: theme.spacing.md,
    paddingVertical: theme.spacing.xs,
    borderRadius: theme.radius.full,
    backgroundColor: theme.colors.surfaceVariant,
  },
  whereText: { color: theme.colors.accent, flexShrink: 1 },
  pressed: { opacity: 0.6 },
}));
