import { useMemo } from 'react';
import { Gesture } from 'react-native-gesture-handler';
import { useSharedValue } from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';
import { haptics } from '@/ui/haptics';
import { dragged, settlesOpen } from './settle';
import { expansion, settlePlayer } from './sheetStore';

/** How far a finger goes before the sheet starts following it. */
const GRAB = 8;

/**
 * A drag that moves the player between its dock and the whole screen, `travel` apart. Each
 * place the sheet can be taken hold of builds its own, as a gesture belongs to one detector.
 * `direction` limits where it starts, so a pull up on the dock opens the player and a pull
 * down on its header closes it, while the other way stays free.
 */
export function useSheetDrag(travel: number, direction: 'up' | 'down') {
  const from = useSharedValue(0);
  // Built once for a travel: a gesture rebuilt under a moving finger is one that lets go.
  return useMemo(
    () =>
      Gesture.Pan()
        .activeOffsetY(direction === 'up' ? -GRAB : GRAB)
        .failOffsetX([-2 * GRAB, 2 * GRAB])
        .onBegin(() => {
          from.value = expansion.value;
        })
        .onUpdate((event) => {
          expansion.value = dragged(from.value, event.translationY, travel);
        })
        .onEnd((event) => {
          scheduleOnRN(haptics.settle);
          // The spring picks up at the finger's speed, in the expansion a second it moves in.
          scheduleOnRN(
            settlePlayer,
            settlesOpen(expansion.value, event.velocityY, travel) ? 1 : 0,
            travel > 0 ? -event.velocityY / travel : 0,
          );
        }),
    [from, travel, direction],
  );
}
