import { useMemo } from 'react';
import { Gesture } from 'react-native-gesture-handler';
import { type SharedValue, withSequence, withTiming } from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';
import { haptics } from '@/ui/haptics';
import { skipFor } from './settle';

/** How far a finger goes sideways before what plays follows it. */
const GRAB = 12;

/**
 * A sideways swipe on what plays: `offset` follows the finger, and let go far or fast enough,
 * what plays slides out that way and the next or previous track slides in from the other
 * side; otherwise it slides back. `skip` sends the command. `width` is how far it slides.
 */
export function useSkipSwipe(
  offset: SharedValue<number>,
  width: number,
  skip: (to: 'next' | 'previous') => void,
) {
  return useMemo(() => {
    const done = (to: 'next' | 'previous') => {
      haptics.confirm();
      skip(to);
    };
    return Gesture.Pan()
      .activeOffsetX([-GRAB, GRAB])
      .failOffsetY([-GRAB, GRAB])
      .onUpdate((event) => {
        offset.value = event.translationX;
      })
      .onEnd((event) => {
        const to = skipFor(event.translationX, event.velocityX, width);
        if (!to) {
          offset.value = withTiming(0, { duration: 200 });
          return;
        }
        const out = to === 'next' ? -width : width;
        offset.value = withSequence(
          withTiming(out, { duration: 160 }),
          withTiming(-out / 3, { duration: 0 }),
          withTiming(0, { duration: 220 }),
        );
        scheduleOnRN(done, to);
      });
  }, [offset, width, skip]);
}
