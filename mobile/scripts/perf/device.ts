/** Talking to an Android device over adb, shared by the scripts in this directory. */

import { execFileSync } from 'node:child_process';

export function adb(serial: string | undefined, args: string[], allowFailure = false): string {
  try {
    return execFileSync('adb', [...(serial ? ['-s', serial] : []), ...args], {
      encoding: 'utf8',
      stdio: ['pipe', 'pipe', 'pipe'],
      maxBuffer: 64 * 1024 * 1024,
    });
  } catch (error) {
    if (allowFailure) return (error as { stdout?: string }).stdout ?? '';
    throw error;
  }
}

export const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export function describeDevice(serial: string | undefined): string {
  const prop = (name: string) => adb(serial, ['shell', `getprop ${name}`]).trim();
  const emulator = prop('ro.kernel.qemu') === '1' || prop('ro.boot.qemu') === '1';
  return `${prop('ro.product.model')} (Android ${prop('ro.build.version.release')}, API ${prop('ro.build.version.sdk')})${emulator ? ' emulator' : ''}`;
}

export function apiLevel(serial: string | undefined): number {
  return Number(adb(serial, ['shell', 'getprop ro.build.version.sdk']).trim());
}

/**
 * A launch behind the lock screen never draws, so the activity manager
 * reports no Displayed time and the app runs paused. Wakes the screen and
 * keeps it on for the rest of the session (a device on USB power stays
 * unlocked that way), dismisses a keyguard that has no credential, and
 * refuses to go on if a lock screen is still up.
 */
export function ensureUnlocked(serial: string | undefined): void {
  adb(serial, ['shell', 'svc power stayon true'], true);
  adb(serial, ['shell', 'input keyevent KEYCODE_WAKEUP'], true);
  adb(serial, ['shell', 'wm dismiss-keyguard'], true);
  const window = adb(serial, ['shell', 'dumpsys window'], true);
  if (/isKeyguardShowing=true|mKeyguardShowing=true/.test(window)) {
    throw new Error(
      'The device is locked. Unlock it (and consider a longer screen timeout) before measuring.',
    );
  }
}

export function installApk(serial: string | undefined, apk: string): void {
  const output = adb(serial, ['install', '-r', '-d', apk]);
  if (!/Success/.test(output)) throw new Error(`adb install failed:\n${output}`);
}
