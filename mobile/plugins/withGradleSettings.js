const { withGradleProperties } = require('expo/config-plugins');

/**
 * Gradle settings this project needs beyond the template's.
 *
 * The heap: the template's two gigabytes leave the daemon garbage-collecting
 * until it gives up once the audio, storage and styling libraries all compile
 * in one build.
 *
 * The build cache: `expo prebuild` regenerates the native directories, and
 * clears them on the way, so every build after one starts with no Gradle
 * output at all. The build cache lives outside the project and survives that,
 * which halves the rebuild.
 */
const PROPERTIES = {
  'org.gradle.jvmargs': '-Xmx6g -XX:MaxMetaspaceSize=1g',
  'org.gradle.caching': 'true',
};

module.exports = (config) =>
  withGradleProperties(config, (mod) => {
    mod.modResults = mod.modResults.filter(
      (item) => !(item.type === 'property' && item.key in PROPERTIES),
    );
    for (const [key, value] of Object.entries(PROPERTIES)) {
      mod.modResults.push({ type: 'property', key, value });
    }
    return mod;
  });
