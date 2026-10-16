# Expo SDK 58 compatibility patches

The workspace applies these patches to both apps through `pnpm-workspace.yaml`:

- `@expo/cli@58.1.10`: SDK 58's remote dependency metadata recommends Jest 29,
  while `jest-expo@58` ships Jest 30 internals and a watch plugin that requires
  Jest 30. Recommend Jest 30 and its types for SDK 58 only. This keeps
  `expo install --fix` from downgrading the workspace's test runner.
- `@expo/metro-config@58.0.14`: preserve the SDK 57 startup optimization that
  uses Metro's default-import helper, allowing inline requires to defer module
  evaluation. Preserve bare import order, live re-exports, and namespace getters.
- `expo-router@58.0.22`: preserve the SDK 57 Android safe-area fix by seeding
  the root with `initialWindowMetrics` before the first native insets event.
- `react-native-gesture-handler@3.3.0`: eagerly load the Android native module
  before the root view reads its JSI module ID. With inline requires, the root can
  otherwise mount before the registry exists and crash on startup.
- `react-native-true-image@2.2.0`: match React Native 0.88's Kotlin bridge
  method names on the custom view delegate. This avoids duplicate JVM methods
  alongside the Java compatibility bridges, including in optimized release builds.

`pnpm test --runInBand scripts/expoPatches.test.ts` from `mobile/` checks Jest
version validation and Metro import behavior against the installed patches.
Android compilation verifies the image delegate patch.

On a future SDK/package upgrade, review each patch against upstream before
removing it or rebasing it onto the new package version.

The app config also keeps Glide's manifest-discovered `OkHttpGlideModule` in
release builds. SDK 58 enables R8 by default; its reflective constructor must
survive shrinking for artwork and palette loading.
