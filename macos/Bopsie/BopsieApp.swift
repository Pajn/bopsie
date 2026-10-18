import SwiftUI

/// Bopsie as a menu bar app: a player on this Mac that a Bopsie server can play to, with its
/// controls in the menu bar and sign-in in the settings window. It has no Dock icon.
@main
struct BopsieApp: App {
    @NSApplicationDelegateAdaptor private var delegate: AppDelegate

    var body: some Scene {
        MenuBarExtra {
            MenuView()
                .environment(delegate.model)
        } label: {
            MenuBarIcon(model: delegate.model)
        }
        .menuBarExtraStyle(.window)

        Settings {
            SettingsView()
                .environment(delegate.model)
        }
    }
}

/// Owns the model, so that the player starts at launch rather than when the menu first opens,
/// and says goodbye to the server when the app quits.
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let model = PlayerModel()

    func applicationDidFinishLaunching(_ notification: Notification) {
        model.resume()
    }

    func applicationWillTerminate(_ notification: Notification) {
        model.shutDown()
    }
}

private struct MenuBarIcon: View {
    let model: PlayerModel

    var body: some View {
        Image(systemName: model.needsSignIn ? "music.note.slash" : "music.note")
            .accessibilityLabel("Bopsie")
    }
}
