/** @type {Detox.DetoxConfig} */
module.exports = {
  testRunner: {
    args: { $0: 'jest', config: 'e2e/jest.config.js' },
    jest: { setupTimeout: 300000 },
  },
  // Release builds embed the JS bundle and leave out the Expo dev launcher,
  // so tests need no Metro and behave the same locally and in CI. Debug
  // builds are development clients: they start on the launcher screen and
  // `e2e/app.ts` sends them the Metro URL after launch. Faster to iterate on.
  // Playback is Android-only for now, so only Android is configured.
  apps: {
    'android.debug': {
      type: 'android.apk',
      binaryPath: 'android/app/build/outputs/apk/debug/app-debug.apk',
      build: 'cd android && ./gradlew assembleDebug assembleAndroidTest -DtestBuildType=debug',
      reversePorts: [8081],
    },
    'android.release': {
      type: 'android.apk',
      binaryPath: 'android/app/build/outputs/apk/release/app-release.apk',
      testBinaryPath:
        'android/app/build/outputs/apk/androidTest/release/app-release-androidTest.apk',
      build: 'cd android && ./gradlew assembleRelease assembleAndroidTest -DtestBuildType=release',
    },
  },
  devices: {
    // E2E_AVD_NAME picks another virtual device.
    emulator: {
      type: 'android.emulator',
      device: { avdName: process.env.E2E_AVD_NAME ?? 'Pixel_8_API_37' },
    },
    // A phone over adb. E2E_ADB_NAME narrows the choice when several are attached.
    attached: { type: 'android.attached', device: { adbName: process.env.E2E_ADB_NAME ?? '.*' } },
  },
  configurations: {
    'android.emu': { device: 'emulator', app: 'android.release' },
    'android.emu.debug': { device: 'emulator', app: 'android.debug' },
    'android.device': { device: 'attached', app: 'android.release' },
  },
  // A player app keeps timers and progress streams alive, so Detox's idle
  // detector never settles. Synchronization is disabled and tests use
  // explicit waitFor assertions instead.
  behavior: {
    init: { exposeGlobals: true, reinstallApp: false },
    launchApp: 'auto',
  },
  session: { debugSynchronization: 0 },
  artifacts: { rootDir: 'artifacts', plugins: { screenshot: 'failing', log: 'failing' } },
};
