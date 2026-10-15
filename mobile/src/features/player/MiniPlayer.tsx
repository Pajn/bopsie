import { useRouter } from 'expo-router';
import { useLayoutEffect } from 'react';
import { Pressable, View } from 'react-native';
import { GestureDetector } from 'react-native-gesture-handler';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { useOutputs } from '@/features/outputs/useOutputs';
import { Icon } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { MINI_COVER, useCoverSlot } from './FlyingCover';
import { nowPlaying } from './nowPlaying';
import { ProgressLine } from './ProgressLine';
import { expandPlayer, setDockHeight } from './sheetStore';
import { usePlaybackActions, useQueue } from './usePlayback';
import { useReportedHeight } from './useReportedHeight';
import { useSheetDrag } from './useSheetDrag';

/**
 * What the active group is playing, above the tab bar: the player sheet docked. A tap opens
 * the full player, and a pull up draws it up the screen; the speaker button opens Play on.
 * Names the group unless it is the phone. `travel` is how far the sheet climbs.
 */
export function MiniPlayer({ travel }: { travel: number }) {
  const router = useRouter();
  const queue = useQueue().data;
  const playback = usePlaybackActions();
  const { queueId: phoneId } = useConnection();
  const active = useOutputs()?.active;
  const current = queue ? nowPlaying(queue) : null;
  const pull = useSheetDrag(travel, 'up');
  const cover = useCoverSlot('mini');
  const measure = useReportedHeight(setDockHeight);
  // With nothing docked the tab bar leaves no gap; a dock that has gone never lays out again
  // to say so.
  const docked = current !== null;
  useLayoutEffect(() => {
    if (!docked) setDockHeight(0);
  }, [docked]);
  if (!queue || !current) return null;
  const playing = queue.state === 'playing';
  const where = active && active.id !== phoneId ? `on ${active.name}` : null;

  return (
    <GestureDetector gesture={pull}>
      <Pressable
        ref={measure.ref}
        onLayout={measure.onLayout}
        onPress={expandPlayer}
        style={styles.bar}
        accessibilityRole="button"
        accessibilityLabel={`Now playing: ${current.title}`}
        testID="mini-player"
      >
        <ProgressLine queue={queue} />
        {/* The artwork is drawn over this, by the sheet, to carry it into the full player. */}
        <View ref={cover.ref} onLayout={cover.onLayout} style={styles.cover} />
        <View style={styles.text}>
          <Text numberOfLines={1} testID="mini-player-title">
            {current.title}
          </Text>
          {where || current.subtitle ? (
            <Text variant="caption" muted numberOfLines={1} testID="mini-player-subtitle">
              {where ?? current.subtitle}
            </Text>
          ) : null}
        </View>
        <Pressable
          onPress={() => router.push('/outputs')}
          hitSlop={12}
          accessibilityRole="button"
          accessibilityLabel="Choose where to play"
          testID="mini-player-outputs"
        >
          <Icon name="speaker" size={22} />
        </Pressable>
        <Pressable
          onPress={() => playback.command('play_pause')}
          hitSlop={12}
          accessibilityRole="button"
          accessibilityLabel={playing ? 'Pause' : 'Play'}
          testID="mini-player-toggle"
        >
          <Icon name={playing ? 'pause' : 'play'} size={26} />
        </Pressable>
      </Pressable>
    </GestureDetector>
  );
}

const styles = StyleSheet.create((theme) => ({
  bar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    paddingHorizontal: theme.spacing.lg,
    paddingVertical: theme.spacing.sm,
    backgroundColor: theme.colors.surface,
    borderTopColor: theme.colors.border,
    borderTopWidth: StyleSheet.hairlineWidth,
  },
  text: { flex: 1, gap: 2 },
  cover: { width: MINI_COVER, height: MINI_COVER },
}));
