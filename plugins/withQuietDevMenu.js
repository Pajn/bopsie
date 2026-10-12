const { AndroidConfig, withAndroidManifest } = require('expo/config-plugins');

/**
 * Keeps the development client's menu out of the way on Android: no
 * onboarding sheet, no menu at launch, no floating button. iOS takes the same
 * flags as launch arguments; Android reads them from manifest meta-data.
 * Only development builds ship the dev menu, so release builds are unaffected.
 */
module.exports = (config) =>
  withAndroidManifest(config, (mod) => {
    const app = AndroidConfig.Manifest.getMainApplicationOrThrow(mod.modResults);
    for (const [name, value] of [
      ['EXDevMenuIsOnboardingFinished', 'true'],
      ['EXDevMenuShowsAtLaunch', 'false'],
      ['EXDevMenuShowFloatingActionButton', 'false'],
    ]) {
      AndroidConfig.Manifest.addMetaDataItemToMainApplication(app, name, value);
    }
    return mod;
  });
