{
  description = "Bopsie development shell (Node, pnpm, JDK, Android SDK, Rust, ffmpeg)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { nixpkgs, ... }:
    let
      systems = [
        "aarch64-darwin"
        "x86_64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];

      # Keep in sync with the Expo SDK's React Native template on upgrade.
      # See docs/plans for the table of pinned Android toolchain versions.
      platformVersion = "36";
      buildToolsVersion = "36.0.0";
      ndkVersion = "27.1.12297006";
      cmakeVersion = "3.22.1";

      forAllSystems = f:
        nixpkgs.lib.genAttrs systems (system:
          f (import nixpkgs {
            inherit system;
            config = {
              # The Android SDK is unfree and its licence must be accepted
              # explicitly before androidenv will fetch it.
              allowUnfree = true;
              android_sdk.accept_license = true;
            };
          }));
    in
    {
      devShells = forAllSystems (pkgs:
        let
          jdk = pkgs.jdk17;

          # The emulator and its system image are a multi-GB download, so they
          # live in a separate shell rather than the default one.
          mkAndroidSdk = { withEmulator }: (pkgs.androidenv.composeAndroidPackages {
            cmdLineToolsVersion = "latest";
            platformToolsVersion = "latest";
            buildToolsVersions = [ buildToolsVersion ];
            platformVersions = [ platformVersion ];
            ndkVersions = [ ndkVersion ];
            cmakeVersions = [ cmakeVersion ];

            includeNDK = true;
            includeCmake = true;
            includeEmulator = withEmulator;
            includeSystemImages = withEmulator;
            systemImageTypes = [ "google_apis" ];
            abiVersions = [
              (if pkgs.stdenv.hostPlatform.isAarch64 then "arm64-v8a" else "x86_64")
            ];

            includeSources = false;
          }).androidsdk;

          mkDevShell = { withEmulator }:
            let
              androidSdk = mkAndroidSdk { inherit withEmulator; };
              sdkRoot = "${androidSdk}/libexec/android-sdk";
            in
            pkgs.mkShell {
              packages = [
                jdk
                androidSdk
                pkgs.nodejs_24
                pkgs.pnpm
                pkgs.watchman
                pkgs.cargo
                pkgs.rustc
                pkgs.clippy
                pkgs.rustfmt
                pkgs.rust-analyzer
                pkgs.ffmpeg
              ];

              JAVA_HOME = jdk.home;
              ANDROID_HOME = sdkRoot;
              ANDROID_SDK_ROOT = sdkRoot;
              ANDROID_NDK_ROOT = "${sdkRoot}/ndk/${ndkVersion}";

              shellHook = ''
                export PATH="${sdkRoot}/platform-tools:$PATH"
              '' + pkgs.lib.optionalString withEmulator ''
                export PATH="${sdkRoot}/emulator:${sdkRoot}/cmdline-tools/latest/bin:$PATH"
              '' + pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
                # AGP's aapt2 comes from Maven as an unpatched ELF binary; use the
                # SDK's instead.
                export GRADLE_OPTS="-Dorg.gradle.project.android.aapt2FromMavenOverride=${sdkRoot}/build-tools/${buildToolsVersion}/aapt2 $GRADLE_OPTS"
              '';
            };
        in
        {
          default = mkDevShell { withEmulator = false; };
          emulator = mkDevShell { withEmulator = true; };
        });
    };
}
