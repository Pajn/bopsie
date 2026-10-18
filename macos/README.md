# Bopsie for Mac

A menu bar app that makes the Mac a Bopsie player: sign in to a Bopsie server in the settings
window, and the server can play to this Mac. The menu bar shows what is playing, with the controls
and this player's volume; Control Center and the media keys work too. It has no Dock icon.

The app is a SwiftUI shell around the [shared Rust player](../player), which it reaches through
UniFFI bindings. The plan behind it is
[the shared Sendspin client plan](../docs/plans/2026-10-18-shared-sendspin-client.md).

## Building

Needs Xcode, and from the development shell `cargo` and `xcodegen`.

```sh
scripts/build-app.sh            # Debug; prints where the app is
scripts/build-app.sh Release
open "$(scripts/build-app.sh)"
```

That runs the three steps, which can also run one at a time:

1. `scripts/build-rust.sh` builds the Rust player as a static library and generates its Swift
   bindings, into `Generated/` (not committed).
2. `xcodegen` generates `Bopsie.xcodeproj` from `project.yml` (not committed either; change the
   YAML instead).
3. `xcodebuild` builds the app into `build/`.

To work in Xcode, run the first two and open `Bopsie.xcodeproj`. Each build runs
`scripts/build-rust.sh` again, so Rust changes are picked up; that needs `cargo` on Xcode's `PATH`,
which it has when Xcode is started from the development shell (`xed .`). Without it the build uses
the Rust player built last.

The app is signed ad hoc, which runs on the Mac that built it, and builds for that Mac's
architecture only.

## Layout

| Path | What |
|---|---|
| `Bopsie/BopsieApp.swift` | The app: the menu bar item and the settings window |
| `Bopsie/PlayerModel.swift` | The account and the Rust player playing for it |
| `Bopsie/AccountStore.swift` | The account and player settings in user defaults, the token in the Keychain |
| `Bopsie/NowPlayingCenter.swift` | Control Center and the media keys |
| `Bopsie/MenuView.swift`, `Bopsie/SettingsView.swift` | The two views |
