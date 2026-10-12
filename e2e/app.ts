import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { device } from 'detox';
import { config as detoxConfig } from 'detox/internals';

/**
 * Set by the debug e2e scripts: the binary is an Expo development client, not
 * a standalone app. Detox does not tell tests which configuration is running.
 */
const DEV_CLIENT = process.env.E2E_DEV_CLIENT === '1';

/**
 * Metro's address as seen from the device. The iOS simulator shares the host's
 * loopback; Android emulators reach it through the reversed port.
 */
const METRO_URL = process.env.E2E_METRO_URL ?? 'http://localhost:8081';

/** Deep link that makes the development client load the bundle from Metro. */
const DEV_CLIENT_URL = `exp+cratie://expo-development-client/?url=${encodeURIComponent(METRO_URL)}`;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Installs the app when the binary on disk is not the one on the device.
 *
 * Detox is told not to reinstall before every suite (see `.detoxrc.js`); a
 * build that has changed since the last run is what warrants an install, and
 * a marker per device remembers which build is on it. Suites wipe the app's
 * data rather than reinstall it, so the install stands between them.
 */
async function installIfBuildChanged(): Promise<void> {
  const binaryPath = (Object.values(detoxConfig.apps)[0] as { binaryPath?: string } | undefined)
    ?.binaryPath;
  if (!binaryPath || !existsSync(binaryPath)) return;
  const stamp = `${binaryPath}@${statSync(binaryPath).mtimeMs}`;
  const marker = join(tmpdir(), `cratie-e2e-installed-${device.id.replace(/[^\w-]+/g, '_')}`);
  if (existsSync(marker) && readFileSync(marker, 'utf8') === stamp) return;
  await device.installApp();
  writeFileSync(marker, stamp);
}

/** The app's id on the device, from the one place the builds take it from. */
function appPackage(): string {
  const appJson = readFileSync(join(__dirname, '..', 'app.json'), 'utf8');
  return (JSON.parse(appJson) as { expo: { android: { package: string } } }).expo.android.package;
}

/**
 * Launches the app with nothing of its own on the device: no session, no
 * downloads, no settings, no dock, and every tab at its root screen.
 *
 * Suites start here so that each one runs the same alone as in a full run.
 * All of that is app data, and undoing it item by item can only undo what a
 * helper remembers to name; wiping it needs no list and leaves nothing.
 * Android clears the data in place and keeps the binary, which costs about a
 * second. The simulator has no such command, so there the app is put back.
 */
export async function freshApp(): Promise<void> {
  await installIfBuildChanged();
  if (device.getPlatform() === 'android') {
    execFileSync('adb', ['-s', device.id, 'shell', 'pm', 'clear', appPackage()], {
      stdio: 'ignore',
    });
  } else {
    await device.uninstallApp();
    await device.installApp();
  }
  await launchApp();
}

/**
 * Launches a fresh instance of the app with synchronization disabled.
 *
 * With a development client, the launch brings up the client's own launcher
 * screen rather than the app. The app only loads once the client is sent a
 * project URL, so everything that waits for the app to render has to come
 * after this returns.
 */
export async function launchApp(extraArgs: Record<string, string | number> = {}): Promise<void> {
  // On Android, Detox's launch only returns once React is running. The dev
  // client will not start React until it has a project URL, so the URL has to
  // travel with the launch intent rather than be sent afterwards.
  const androidDevClient = DEV_CLIENT && device.getPlatform() === 'android';
  await installIfBuildChanged();
  await device.launchApp({
    newInstance: true,
    ...(androidDevClient ? { url: DEV_CLIENT_URL } : {}),
    launchArgs: {
      detoxEnableSynchronization: 0,
      // iOS reads `-Key value` process arguments into NSUserDefaults, where
      // the dev menu keeps its flags. Keeps its onboarding sheet and floating
      // button out of the way in development clients.
      EXDevMenuIsOnboardingFinished: 1,
      EXDevMenuShowsAtLaunch: 0,
      EXDevMenuShowFloatingActionButton: 0,
      ...extraArgs,
    },
  });
  if (DEV_CLIENT && !androidDevClient) {
    await sleep(1500);
    openDeepLink(DEV_CLIENT_URL);
  }
}

/**
 * Opens `url` on the device the way the OS would.
 *
 * Detox's own `device.openURL` mocks the delivery inside the app process, and
 * the Expo development client never sees it. The platform tools deliver a
 * real launch URL that it does handle.
 */
export function openDeepLink(url: string): void {
  if (device.getPlatform() === 'ios') {
    execFileSync('xcrun', ['simctl', 'openurl', device.id, url], { stdio: 'ignore' });
  } else {
    execFileSync(
      'adb',
      ['-s', device.id, 'shell', 'am', 'start', '-a', 'android.intent.action.VIEW', '-d', url],
      { stdio: 'ignore' },
    );
  }
}
