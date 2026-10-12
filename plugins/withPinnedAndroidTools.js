const { withProjectBuildGradle } = require('expo/config-plugins');

/**
 * Makes every Android library module build with the project's NDK and
 * build tools.
 *
 * Modules that set neither fall back to the Android Gradle plugin's defaults
 * and ask the SDK to install them, which fails wherever the SDK is read-only,
 * as it is in the flake's development shell. The project's versions are the
 * ones React Native's Gradle plugin publishes on `rootProject.ext`; the flake
 * pins the same ones.
 */
module.exports = (config) =>
  withProjectBuildGradle(config, (mod) => {
    const marker = 'withPinnedAndroidTools';
    if (!mod.modResults.contents.includes(marker)) {
      mod.modResults.contents += `
// ${marker}: library modules without their own NDK or build tools use the project's.
subprojects { project ->
  project.plugins.withId('com.android.library') {
    project.android.ndkVersion = rootProject.ext.ndkVersion
    project.android.buildToolsVersion = rootProject.ext.buildToolsVersion
  }
}
`;
    }
    return mod;
  });
