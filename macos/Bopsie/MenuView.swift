import SwiftUI

/// The menu bar window: what is playing, the controls, and the way to settings.
struct MenuView: View {
    @Environment(PlayerModel.self) private var model

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if model.account == nil {
                Text("Sign in to a Bopsie server to play here.")
                    .foregroundStyle(.secondary)
            } else {
                status
                if let snapshot = model.snapshot, snapshot.connection == .connected {
                    nowPlaying(snapshot.nowPlaying)
                    transport
                    volume(snapshot)
                }
            }
            Divider()
            HStack {
                SettingsLink {
                    Text(model.needsSignIn ? "Sign In…" : "Settings…")
                }
                .simultaneousGesture(TapGesture().onEnded { NSApp.activate() })
                Spacer()
                Button("Quit Bopsie") { NSApp.terminate(nil) }
            }
        }
        .padding(14)
        .frame(width: 300)
    }

    @ViewBuilder
    private var status: some View {
        switch model.snapshot?.connection {
        case .connected:
            EmptyView()
        case .reconnecting(_, let error):
            Label("Reconnecting: \(error)", systemImage: "wifi.exclamationmark")
                .foregroundStyle(.secondary)
                .lineLimit(2)
        case .unauthorized:
            Label("The server no longer accepts this sign-in.", systemImage: "person.crop.circle.badge.exclamationmark")
                .foregroundStyle(.secondary)
        case .connecting, .stopped, nil:
            Label("Connecting…", systemImage: "antenna.radiowaves.left.and.right")
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func nowPlaying(_ now: NowPlaying?) -> some View {
        HStack(spacing: 10) {
            AsyncImage(url: now?.artworkUrl.flatMap(URL.init(string:))) { image in
                image.resizable().scaledToFill()
            } placeholder: {
                Image(systemName: "music.note")
                    .foregroundStyle(.tertiary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(.quaternary)
            }
            .frame(width: 48, height: 48)
            .clipShape(RoundedRectangle(cornerRadius: 6))

            VStack(alignment: .leading, spacing: 2) {
                Text(now?.title ?? "Nothing playing")
                    .font(.headline)
                    .lineLimit(1)
                if let line = [now?.artist, now?.album].compactMap({ $0 }).joined(separator: " — ").nilIfEmpty {
                    Text(line)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
        }
    }

    private var transport: some View {
        HStack(spacing: 24) {
            Spacer()
            Button { model.send(.previous) } label: {
                Image(systemName: "backward.fill")
            }
            .disabled(!model.canSend("previous"))
            Button { model.playPause() } label: {
                Image(systemName: model.isPlaying ? "pause.fill" : "play.fill")
                    .font(.title2)
            }
            .disabled(!model.canSend(model.isPlaying ? "pause" : "play"))
            Button { model.send(.next) } label: {
                Image(systemName: "forward.fill")
            }
            .disabled(!model.canSend("next"))
            Spacer()
        }
        .buttonStyle(.plain)
        .font(.title3)
    }

    private func volume(_ snapshot: Snapshot) -> some View {
        HStack {
            Image(systemName: "speaker.fill").foregroundStyle(.secondary)
            Slider(
                value: Binding(
                    get: { Double(snapshot.volume) },
                    set: { model.setVolume($0) }
                ),
                in: 0...100
            )
            Image(systemName: "speaker.wave.3.fill").foregroundStyle(.secondary)
        }
    }
}

private extension String {
    var nilIfEmpty: String? { isEmpty ? nil : self }
}
