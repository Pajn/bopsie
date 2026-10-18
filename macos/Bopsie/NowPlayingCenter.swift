import MediaPlayer

/// Shows what is playing in Control Center and answers the media keys, by sending the commands
/// to the player's group.
@MainActor
final class NowPlayingCenter {
    var onCommand: ((Remote) -> Void)?

    private let info = MPNowPlayingInfoCenter.default()
    private var isPlaying = false

    init() {
        let commands = MPRemoteCommandCenter.shared()
        handle(commands.playCommand) { $0.onCommand?(.play) }
        handle(commands.pauseCommand) { $0.onCommand?(.pause) }
        handle(commands.togglePlayPauseCommand) { center in
            center.onCommand?(center.isPlaying ? .pause : .play)
        }
        handle(commands.nextTrackCommand) { $0.onCommand?(.next) }
        handle(commands.previousTrackCommand) { $0.onCommand?(.previous) }
    }

    func update(_ snapshot: Snapshot?) {
        isPlaying = snapshot?.playing ?? false
        guard let snapshot, snapshot.connection == .connected, let now = snapshot.nowPlaying else {
            info.nowPlayingInfo = nil
            info.playbackState = .stopped
            return
        }
        var properties: [String: Any] = [
            MPMediaItemPropertyTitle: now.title ?? "",
            MPMediaItemPropertyArtist: now.artist ?? "",
            MPMediaItemPropertyAlbumTitle: now.album ?? "",
        ]
        if let progress = now.progress {
            let elapsed = Double(Date().timeIntervalSince1970 * 1000) - Double(progress.receivedAtMs)
            let position = Double(progress.positionMs) + max(0, elapsed) * progress.speed
            properties[MPMediaItemPropertyPlaybackDuration] = Double(progress.durationMs) / 1000
            properties[MPNowPlayingInfoPropertyElapsedPlaybackTime] = position / 1000
            properties[MPNowPlayingInfoPropertyPlaybackRate] = progress.speed
        }
        info.nowPlayingInfo = properties
        info.playbackState = snapshot.playing ? .playing : .paused
    }

    private func handle(
        _ command: MPRemoteCommand,
        _ action: @escaping @MainActor (NowPlayingCenter) -> Void
    ) {
        command.addTarget { [weak self] _ in
            guard let self else { return .commandFailed }
            MainActor.assumeIsolated { action(self) }
            return .success
        }
    }
}
