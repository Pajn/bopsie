import { useRouter } from 'expo-router';
import { MenuRow, MenuSheet, MenuTitle } from '@/features/library/menu';
import { usePlaybackActions, useQueue } from '@/features/player/usePlayback';
import { SLEEP_MINUTES, SPEEDS, sleeping, speedLabel } from './model';

/** How fast the book playing plays: one of the speeds, the current one checked. */
export function SpeedMenu() {
  const router = useRouter();
  const queue = useQueue().data;
  const playback = usePlaybackActions();
  const current = queue?.playback_speed ?? 1;
  return (
    <MenuSheet testID="speed-menu">
      <MenuTitle title="Playback speed" />
      {SPEEDS.map((speed) => (
        <MenuRow
          key={speed}
          label={speedLabel(speed)}
          selected={Math.abs(speed - current) < 0.001}
          onPress={() => {
            router.back();
            void playback.speed(speed);
          }}
          testID={`speed-${speed}`}
        />
      ))}
    </MenuSheet>
  );
}

/** When to pause: in some minutes, at the end of the chapter, or not at all. */
export function SleepMenu() {
  const router = useRouter();
  const queue = useQueue().data;
  const playback = usePlaybackActions();
  const choose = (timer: Parameters<typeof playback.sleep>[0]) => {
    router.back();
    void playback.sleep(timer);
  };
  return (
    <MenuSheet testID="sleep-menu">
      <MenuTitle title="Sleep timer" />
      {SLEEP_MINUTES.map((minutes) => (
        <MenuRow
          key={minutes}
          icon="clock"
          label={`In ${minutes} minutes`}
          onPress={() => choose({ minutes })}
          testID={`sleep-${minutes}`}
        />
      ))}
      <MenuRow
        icon="bookmark"
        label="At the end of the chapter"
        onPress={() => choose('end_of_chapter')}
        testID="sleep-end-of-chapter"
      />
      {queue && sleeping(queue) ? (
        <MenuRow icon="x" label="Turn off" onPress={() => choose(null)} testID="sleep-off" />
      ) : null}
    </MenuSheet>
  );
}
