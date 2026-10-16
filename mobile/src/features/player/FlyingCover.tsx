import type { WithImages } from '@bopsie/client/api/schemas/media';
import {
  createContext,
  type ReactNode,
  type RefObject,
  useCallback,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import type { ViewInstance } from 'react-native';
import Animated, { interpolate, useAnimatedStyle } from 'react-native-reanimated';
import { StyleSheet } from 'react-native-unistyles';
import { Artwork } from '@/features/library/Artwork';
import { coverMotion, type Square } from './coverMotion';
import { coverSwipe, expansion, panelCoverage } from './sheetStore';

/** The artwork's sizes in the mini player and the full player. */
export const MINI_COVER = 40;
export const FULL_COVER = 300;
const RADIUS = 6;

type Slot = 'mini' | 'full';

interface CoverPlaces {
  /** The sheet's frame, which every place is measured against. */
  frame: RefObject<ViewInstance | null>;
  report: (slot: Slot, square: Square) => void;
}

const CoverContext = createContext<CoverPlaces | null>(null);

/**
 * Holds where the artwork sits in the mini player and in the full player, as the two report
 * it, so `FlyingCover` can carry it between them. `frame` is the view they measure against.
 */
export function useCoverPlaces(frame: RefObject<ViewInstance | null>) {
  const [places, setPlaces] = useState<Partial<Record<Slot, Square>>>({});
  const report = useCallback((slot: Slot, square: Square) => {
    setPlaces((current) => {
      const last = current[slot];
      return last && last.x === square.x && last.y === square.y && last.size === square.size
        ? current
        : { ...current, [slot]: square };
    });
  }, []);
  const value = useMemo(() => ({ frame, report }), [frame, report]);
  return { places, value };
}

export function CoverPlacesProvider({
  value,
  children,
}: {
  value: CoverPlaces;
  children: ReactNode;
}) {
  return <CoverContext.Provider value={value}>{children}</CoverContext.Provider>;
}

/**
 * A place the artwork is drawn over rather than in: attach the ref to an empty view of the
 * artwork's size. It reports where it sits after every render, as anything above it can move
 * it, and when laid out.
 */
export function useCoverSlot(slot: Slot) {
  const places = useContext(CoverContext);
  const ref = useRef<ViewInstance>(null);
  const measure = useCallback(() => {
    const frame = places?.frame.current;
    if (!places || !frame || !ref.current) return;
    ref.current.measureLayout(frame, (x, y, width) => places.report(slot, { x, y, size: width }));
  }, [places, slot]);
  useLayoutEffect(measure);
  return { ref, onLayout: measure };
}

/**
 * The artwork of what plays, drawn once above the whole player sheet and carried, as the sheet
 * opens, from its place in the mini player to its place in the full player, growing on the
 * way: the one piece that stays in view throughout. It fades under a panel growing over the
 * player, and takes no touches, which go to what is beneath it.
 */
export function FlyingCover({
  artwork,
  mini,
  full,
}: {
  artwork: WithImages;
  mini: Square;
  full: Square;
}) {
  const style = useAnimatedStyle(() => {
    const motion = coverMotion(mini, full, expansion.value, RADIUS);
    // A swipe on the full player's artwork carries it sideways; docked, nothing does.
    const swipe = coverSwipe.value * expansion.value;
    return {
      opacity:
        interpolate(panelCoverage.value, [0, 0.3], [1, 0], 'clamp') *
        interpolate(Math.abs(swipe), [0, full.size], [1, 0.2], 'clamp'),
      borderRadius: motion.borderRadius,
      transform: [
        { translateX: motion.translateX + swipe },
        { translateY: motion.translateY },
        { scale: motion.scale },
      ],
    };
  });
  return (
    <Animated.View
      pointerEvents="none"
      style={[
        styles.cover,
        { left: full.x, top: full.y, width: full.size, height: full.size },
        style,
      ]}
    >
      <Artwork item={artwork} size={full.size} imageSize={1024} />
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  cover: { position: 'absolute', overflow: 'hidden' },
});
