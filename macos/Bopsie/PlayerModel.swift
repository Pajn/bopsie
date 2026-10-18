import Foundation
import Observation

/// The app's state: the account, and the Rust player playing for it.
@MainActor
@Observable
final class PlayerModel {
    private(set) var account: Account?
    private(set) var snapshot: Snapshot?

    @ObservationIgnored private var player: PlayerHandle?
    /// Counts players started, so a snapshot from one that was replaced is ignored.
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private let store = AccountStore()
    @ObservationIgnored private let nowPlayingCenter = NowPlayingCenter()

    init() {
        initLogging(filter: "warn,bopsie_player=info")
        account = store.load()
        nowPlayingCenter.onCommand = { [weak self] remote in self?.send(remote) }
    }

    /// Whether the user has to sign in: never signed in, or the server stopped accepting it.
    var needsSignIn: Bool {
        account == nil || snapshot?.connection == .unauthorized
    }

    var isPlaying: Bool { snapshot?.playing ?? false }

    /// Starts playing for the stored account, at launch.
    func resume() {
        guard account != nil, player == nil else { return }
        start()
    }

    func logIn(server: String, username: String, password: String, playerName: String) async throws {
        let name = playerName.trimmingCharacters(in: .whitespaces)
        let signedIn = try await signIn(
            serverUrl: server,
            username: username,
            password: password,
            deviceName: "\(name) (Bopsie for Mac)"
        )
        let account = Account(
            serverURL: signedIn.serverUrl,
            username: signedIn.username,
            playerName: name
        )
        try store.save(account, token: signedIn.token)
        stopPlayer()
        self.account = account
        start()
    }

    func signOut() {
        stopPlayer()
        store.clear()
        account = nil
    }

    /// Stops the player and waits for its goodbye to reach the server, as the app quits.
    func shutDown() {
        player?.stop()
        player = nil
    }

    func send(_ remote: Remote) {
        player?.remote(remote: remote)
    }

    func playPause() {
        send(isPlaying ? .pause : .play)
    }

    /// Sets this player's own volume, 0 to 100.
    func setVolume(_ volume: Double) {
        let volume = UInt8(clamping: Int(volume.rounded()))
        player?.setVolume(volume: volume)
    }

    func canSend(_ command: String) -> Bool {
        snapshot?.controls?.supportedCommands.contains(command) ?? false
    }

    private func start() {
        guard let account, let token = store.token() else {
            account = nil
            return
        }
        generation += 1
        let generation = generation
        let config = PlayerConfig(
            serverUrl: account.serverURL,
            token: token,
            clientId: store.clientID,
            name: account.playerName,
            productName: desktopProductName(),
            softwareVersion: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String,
            volume: store.volume,
            muted: false
        )
        let observer = Observer { [weak self] snapshot in
            Task { @MainActor in self?.apply(snapshot, from: generation) }
        }
        do {
            player = try PlayerHandle.start(config: config, observer: observer)
        } catch {
            snapshot = Snapshot(
                connection: .reconnecting(attempt: 0, error: userMessage(for: error)),
                playing: false, nowPlaying: nil, controls: nil, volume: store.volume, muted: false
            )
        }
    }

    private func stopPlayer() {
        generation += 1
        snapshot = nil
        nowPlayingCenter.update(nil)
        guard let player else { return }
        self.player = nil
        // Stopping waits for the goodbye to reach the server; not on the main thread.
        Task.detached { player.stop() }
    }

    private func apply(_ snapshot: Snapshot, from generation: Int) {
        guard generation == self.generation else { return }
        self.snapshot = snapshot
        if snapshot.volume != store.volume {
            store.volume = snapshot.volume
        }
        nowPlayingCenter.update(snapshot)
    }
}

/// Hands the player's snapshots, which arrive on a thread of the player's, to a closure.
private final class Observer: PlayerObserver {
    private let onChange: @Sendable (Snapshot) -> Void

    init(_ onChange: @escaping @Sendable (Snapshot) -> Void) {
        self.onChange = onChange
    }

    func changed(snapshot: Snapshot) {
        onChange(snapshot)
    }
}

/// What to tell a person about an error, in place of the bindings' debug description.
func userMessage(for error: Error) -> String {
    guard let error = error as? PlayerError else { return error.localizedDescription }
    switch error {
    case let .InvalidAddress(message), let .Connection(message), let .Other(message):
        return message
    case let .WrongCredentials(message):
        return message.isEmpty ? "Wrong username or password." : message
    case .SetupRequired:
        return "This server has not been set up yet. Set it up in its web interface first."
    case .ServerTooOld:
        return "This server is too old for this version of Bopsie."
    case .Unauthorized:
        return "The server no longer accepts this sign-in."
    }
}
