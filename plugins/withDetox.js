const fs = require('node:fs');
const path = require('node:path');
const {
  withAppBuildGradle,
  withDangerousMod,
  withProjectBuildGradle,
} = require('expo/config-plugins');

/**
 * Wires Detox into the prebuilt Android project: its Maven repository, the
 * instrumentation runner and test dependency, and the JUnit entry point.
 * The iOS side needs nothing; Detox drives the app from outside there.
 */
const withDetoxTestRepo = (config) =>
  withProjectBuildGradle(config, (mod) => {
    const marker = 'Detox-android';
    if (!mod.modResults.contents.includes(marker)) {
      mod.modResults.contents = mod.modResults.contents.replace(
        /allprojects\s*\{\s*repositories\s*\{/,
        (match) => `${match}\n    maven { url("$rootDir/../node_modules/detox/Detox-android") }`,
      );
    }
    return mod;
  });

const withDetoxAppGradle = (config) =>
  withAppBuildGradle(config, (mod) => {
    let contents = mod.modResults.contents;
    if (!contents.includes('testInstrumentationRunner')) {
      contents = contents.replace(
        /defaultConfig\s*\{/,
        (match) =>
          `${match}\n        testBuildType System.getProperty('testBuildType', 'debug')\n        testInstrumentationRunner 'androidx.test.runner.AndroidJUnitRunner'`,
      );
    }
    if (!contents.includes('com.wix:detox')) {
      contents = contents.replace(
        /dependencies\s*\{/,
        (match) =>
          `${match}
    androidTestImplementation('com.wix:detox:+')
    androidTestImplementation('androidx.test:runner:1.6.2')
    androidTestImplementation('androidx.test:rules:1.6.1')
    androidTestImplementation('androidx.test:core:1.6.1')
    androidTestImplementation('androidx.test:monitor:1.7.2')
    androidTestImplementation('androidx.test.ext:junit:1.2.1')
    implementation('androidx.appcompat:appcompat:1.7.0')`,
      );
    }
    if (!contents.includes('withDetox: resolutionStrategy')) {
      // The development client puts an older AndroidX test runner on the app
      // classpath, and Gradle holds the test classpath to the same version.
      // Detox's runner needs the newer family, so both classpaths get it.
      contents += `
// withDetox: resolutionStrategy
configurations.all {
  resolutionStrategy.force(
    'androidx.test:runner:1.6.2',
    'androidx.test:rules:1.6.1',
    'androidx.test:core:1.6.1',
    'androidx.test:monitor:1.7.2',
    'androidx.test.ext:junit:1.2.1'
  )
}
`;
    }
    if (!contents.includes("androidTestImplementation('androidx.media3:media3-session")) {
      contents = contents.replace(
        /dependencies\s*\{/,
        (match) =>
          `${match}\n    androidTestImplementation('androidx.media3:media3-session:1.11.1')`,
      );
    }
    mod.modResults.contents = contents;
    return mod;
  });

const withDetoxTestClass = (config) =>
  withDangerousMod(config, [
    'android',
    (mod) => {
      const pkg = mod.android?.package;
      if (!pkg) throw new Error('withDetox: android.package is required');
      const dir = path.join(
        mod.modRequest.platformProjectRoot,
        'app/src/androidTest/java',
        ...pkg.split('.'),
      );
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(
        path.join(dir, 'DetoxTest.java'),
        `package ${pkg};

import com.wix.detox.Detox;
import com.wix.detox.config.DetoxConfig;

import org.junit.Rule;
import org.junit.Test;
import org.junit.runner.RunWith;

import androidx.test.ext.junit.runners.AndroidJUnit4;
import androidx.test.filters.LargeTest;
import androidx.test.rule.ActivityTestRule;

@RunWith(AndroidJUnit4.class)
@LargeTest
public class DetoxTest {
    @Rule
    public ActivityTestRule<MainActivity> mActivityRule = new ActivityTestRule<>(MainActivity.class, false, false);

    @Test
    public void runDetoxTests() {
        DetoxConfig detoxConfig = new DetoxConfig();
        detoxConfig.idlePolicyConfig.masterTimeoutSec = 90;
        detoxConfig.idlePolicyConfig.idleResourceTimeoutSec = 60;
        detoxConfig.rnContextLoadTimeoutSec = 180;
        Detox.runTests(mActivityRule, detoxConfig);
    }
}
`,
      );
      return mod;
    },
  ]);

module.exports = (config) => withDetoxTestClass(withDetoxAppGradle(withDetoxTestRepo(config)));
