const fs = require('node:fs/promises');
const path = require('node:path');
const { AndroidConfig, withAndroidManifest } = require('expo/config-plugins');

/**
 * Self-hosted servers can use a CA installed by the user. Android apps targeting
 * API 24+ must opt into that trust store; the platform still validates the
 * certificate chain and hostname. Register before expo-build-properties so
 * its manifest mod runs first and supplies the app's existing HTTP policy.
 */
module.exports = (config) =>
  withAndroidManifest(config, async (mod) => {
    const app = AndroidConfig.Manifest.getMainApplicationOrThrow(mod.modResults);
    // A network security config supersedes usesCleartextTraffic, so carry the
    // existing policy into it rather than changing HTTP support as a side effect.
    const cleartext = app.$['android:usesCleartextTraffic'] === 'true';
    const dir = path.join(mod.modRequest.platformProjectRoot, 'app/src/main/res/xml');
    await fs.mkdir(dir, { recursive: true });
    await fs.writeFile(
      path.join(dir, 'cratie_network_security_config.xml'),
      `<?xml version="1.0" encoding="utf-8"?>
<network-security-config>
    <base-config cleartextTrafficPermitted="${cleartext}">
        <trust-anchors>
            <certificates src="system" />
            <certificates src="user" />
        </trust-anchors>
    </base-config>
</network-security-config>
`,
    );
    app.$['android:networkSecurityConfig'] = '@xml/cratie_network_security_config';
    return mod;
  });
