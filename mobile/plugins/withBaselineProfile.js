const fs = require('node:fs');
const path = require('node:path');
const { withDangerousMod } = require('expo/config-plugins');

/**
 * Ships the baseline profile with the app.
 *
 * A baseline profile names the classes and methods a launch needs. Android
 * compiles them and lays them out together when the app is installed, rather
 * than learning it over the user's first several launches, which is where a
 * cold start otherwise spends much of its time loading and linking classes.
 *
 * `scripts/perf/baseline-profile.ts` records the file; the Android Gradle
 * plugin reads it from the main source set, and `androidx.profileinstaller`,
 * which the app already depends on, applies it on a device that installs the
 * APK directly. A checkout without a recorded profile builds without one.
 */
const SOURCE = 'profiles/baseline-prof.txt';

module.exports = (config) =>
  withDangerousMod(config, [
    'android',
    (mod) => {
      const source = path.join(mod.modRequest.projectRoot, SOURCE);
      const target = path.join(
        mod.modRequest.platformProjectRoot,
        'app/src/main/baseline-prof.txt',
      );
      if (!fs.existsSync(source)) return mod;
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.copyFileSync(source, target);
      return mod;
    },
  ]);
