import * as Haptics from 'expo-haptics';
import { Platform } from 'react-native';

/**
 * The app's haptics, by what happened rather than how it feels. Android has the device's own
 * haptic patterns for these, which follow its settings; iOS has the Taptic Engine's.
 */
const android = (type: Haptics.AndroidHaptics) => () =>
  void Haptics.performAndroidHapticsAsync(type).catch(() => {});

export const haptics =
  Platform.OS === 'android'
    ? {
        /** A long press that opened a menu. */
        longPress: android(Haptics.AndroidHaptics.Long_Press),
        /** Something switched on or off, such as shuffle. */
        toggle: (on: boolean) =>
          android(on ? Haptics.AndroidHaptics.Toggle_On : Haptics.AndroidHaptics.Toggle_Off)(),
        /** A drag let go of that settles something, such as the player sheet. */
        settle: android(Haptics.AndroidHaptics.Gesture_End),
        /** A swipe that did what it was for, such as skipping a track. */
        confirm: android(Haptics.AndroidHaptics.Confirm),
      }
    : {
        longPress: () =>
          void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Medium).catch(() => {}),
        toggle: (_on: boolean) => void Haptics.selectionAsync().catch(() => {}),
        settle: () => void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light).catch(() => {}),
        confirm: () => void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Rigid).catch(() => {}),
      };
