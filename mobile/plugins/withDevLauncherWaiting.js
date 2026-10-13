const { AndroidConfig, withAndroidManifest, withInfoPlist } = require('expo/config-plugins');

/**
 * Makes the development client wait on its launcher screen instead of loading
 * the project it last opened. Automated runs send it the project URL after
 * launch; a client that has already started the last project reloads on that
 * URL, and the app starts twice in one process.
 *
 * Writes the same settings the dev-launcher plugin's `launchMode: 'launcher'`
 * does. That plugin is applied once, by the dev client's own plugin, before
 * anything in the app config runs, so options given to it there are dropped.
 */
module.exports = (config) => {
  config = withInfoPlist(config, (mod) => {
    mod.modResults.DEV_CLIENT_TRY_TO_LAUNCH_LAST_BUNDLE = false;
    return mod;
  });
  return withAndroidManifest(config, (mod) => {
    const app = AndroidConfig.Manifest.getMainApplicationOrThrow(mod.modResults);
    AndroidConfig.Manifest.addMetaDataItemToMainApplication(
      app,
      'DEV_CLIENT_TRY_TO_LAUNCH_LAST_BUNDLE',
      'false',
    );
    return mod;
  });
};
