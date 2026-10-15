import { type ComponentRef, useCallback, useLayoutEffect, useRef } from 'react';
import type { LayoutChangeEvent, View } from 'react-native';

/**
 * Hands `report` the height of the view it is attached to, in time for the frame that view
 * first appears in.
 *
 * `onLayout` alone arrives once a frame is already drawn, so a layout built from what it
 * measures is drawn wrong first and right a frame later, once for each measurement it waits
 * on. A layout effect reads the layout before anything is shown, and what it sets is drawn in
 * that same frame. `onLayout` stays for changes no render of this view caused.
 */
export function useReportedHeight(report: (height: number) => void) {
  const ref = useRef<ComponentRef<typeof View>>(null);
  useLayoutEffect(() => {
    const height = ref.current?.getBoundingClientRect().height;
    if (height !== undefined) report(height);
  });
  const onLayout = useCallback(
    (event: LayoutChangeEvent) => report(event.nativeEvent.layout.height),
    [report],
  );
  return { ref, onLayout };
}
