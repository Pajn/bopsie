import { makeMutable, withSpring } from 'react-native-reanimated';
import { useStore } from 'zustand';
import { createStore } from 'zustand/vanilla';

/**
 * How far the player is out of its dock: 0 docked above the tab bar as the mini player, 1
 * covering the screen.
 *
 * A finger writes to this every frame it moves, so it lives on the UI thread rather than in
 * the store below, which carries only what JS has to know and changes once a gesture.
 */
export const expansion = makeMutable(0);

/**
 * How far a panel in the player, such as the queue, has grown over it, from 0 to 1. The
 * artwork, which is drawn above the player so it can fly in from the mini player, fades
 * under a panel by it.
 */
export const panelCoverage = makeMutable(0);

/** How far the full player's artwork is swiped sideways, for the flying artwork to follow. */
export const coverSwipe = makeMutable(0);

/**
 * Firm enough to feel placed rather than thrown. Critically damped, at 2√(stiffness × mass),
 * so it arrives without sailing past; the clamp keeps it so if the others are nudged.
 */
const SPRING = { damping: 26, stiffness: 240, mass: 0.7, overshootClamping: true } as const;

interface SheetState {
  /** Whether the player is up, or on its way up. */
  open: boolean;
  /** What the dock measures, so the tab bar can leave a gap the same size. */
  dockHeight: number;
  /** The dock's gap and the bar together, which is how far up the dock sits. */
  tabBarHeight: number;
}

const store = createStore<SheetState>(() => ({ open: false, dockHeight: 0, tabBarHeight: 0 }));

/**
 * Puts the player at one end or the other, carrying on at the speed a finger let go at, in
 * expansion a second, so the hand-over has no seam.
 */
export function settlePlayer(target: 0 | 1, velocity = 0): void {
  store.setState({ open: target === 1 });
  expansion.value = withSpring(target, { ...SPRING, velocity });
}

export function expandPlayer(): void {
  settlePlayer(1);
}

export function collapsePlayer(): void {
  settlePlayer(0);
}

export function setDockHeight(height: number): void {
  if (store.getState().dockHeight !== height) store.setState({ dockHeight: height });
}

export function setTabBarHeight(height: number): void {
  if (store.getState().tabBarHeight !== height) store.setState({ tabBarHeight: height });
}

export const usePlayerOpen = (): boolean => useStore(store, (s) => s.open);
export const useDockHeight = (): number => useStore(store, (s) => s.dockHeight);
export const useTabBarHeight = (): number => useStore(store, (s) => s.tabBarHeight);
