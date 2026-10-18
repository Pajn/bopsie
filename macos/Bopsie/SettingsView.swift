import ServiceManagement
import SwiftUI

/// The settings window: signing in and out, and starting at login.
struct SettingsView: View {
    @Environment(PlayerModel.self) private var model

    var body: some View {
        Form {
            if let account = model.account {
                SignedInSection(account: account)
            } else {
                SignInSection()
            }
            Section {
                LaunchAtLoginToggle()
            }
        }
        .formStyle(.grouped)
        .frame(width: 420)
        .fixedSize(horizontal: false, vertical: true)
    }
}

private struct SignedInSection: View {
    @Environment(PlayerModel.self) private var model
    let account: Account

    var body: some View {
        Section("Account") {
            LabeledContent("Server", value: account.serverURL)
            LabeledContent("Username", value: account.username)
            LabeledContent("Player name", value: account.playerName)
            if model.snapshot?.connection == .unauthorized {
                Text("The server no longer accepts this sign-in. Sign out and sign in again.")
                    .foregroundStyle(.secondary)
            }
            Button("Sign Out", role: .destructive) { model.signOut() }
        }
    }
}

private struct SignInSection: View {
    @Environment(PlayerModel.self) private var model
    @State private var server = ""
    @State private var username = ""
    @State private var password = ""
    @State private var playerName = Host.current().localizedName ?? "Mac"
    @State private var signingIn = false
    @State private var error: String?

    var body: some View {
        Section {
            TextField("Server", text: $server, prompt: Text("music.local or https://music.example.com"))
                .textContentType(.URL)
            TextField("Username", text: $username)
                .textContentType(.username)
            SecureField("Password", text: $password)
                .textContentType(.password)
            TextField("Player name", text: $playerName)
        } header: {
            Text("Sign in to a Bopsie server")
        } footer: {
            VStack(alignment: .leading, spacing: 8) {
                if let error {
                    Text(error).foregroundStyle(.red)
                }
                HStack {
                    Spacer()
                    if signingIn {
                        ProgressView().controlSize(.small)
                    }
                    Button("Sign In") { submit() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(!canSubmit)
                }
            }
        }
        .onSubmit { if canSubmit { submit() } }
    }

    private var canSubmit: Bool {
        !signingIn && !server.trimmingCharacters(in: .whitespaces).isEmpty && !username.isEmpty
            && !password.isEmpty && !playerName.trimmingCharacters(in: .whitespaces).isEmpty
    }

    private func submit() {
        signingIn = true
        error = nil
        Task {
            defer { signingIn = false }
            do {
                try await model.logIn(
                    server: server,
                    username: username,
                    password: password,
                    playerName: playerName
                )
                password = ""
            } catch {
                self.error = userMessage(for: error)
            }
        }
    }
}

private struct LaunchAtLoginToggle: View {
    @State private var enabled = SMAppService.mainApp.status == .enabled
    @State private var error: String?

    var body: some View {
        Toggle("Open Bopsie at login", isOn: Binding(get: { enabled }, set: set))
        if let error {
            Text(error).foregroundStyle(.secondary)
        }
    }

    private func set(_ enable: Bool) {
        do {
            if enable {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
        enabled = SMAppService.mainApp.status == .enabled
    }
}
