import AppKit
import MediaPlayer

/// Shows what is playing in Control Center and answers the media keys, by sending the commands
/// to the player's group.
@MainActor
final class NowPlayingCenter {
    var onCommand: ((Remote) -> Void)?

    private let info = MPNowPlayingInfoCenter.default()
    private var isPlaying = false
    /// The artwork shown, and the address it came from: fetched once per track, not per update.
    private var artworkURL: String?
    private var artwork: MPMediaItemArtwork?
    private var artworkTask: Task<Void, Never>?

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
            loadArtwork(from: nil)
            return
        }
        loadArtwork(from: now.artworkUrl)
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
        if let artwork {
            properties[MPMediaItemPropertyArtwork] = artwork
        }
        info.nowPlayingInfo = properties
        info.playbackState = snapshot.playing ? .playing : .paused
    }

    /// Fetches the artwork at `url` when it is not the one shown, and adds it to what is showing
    /// once it arrives.
    private func loadArtwork(from url: String?) {
        guard url != artworkURL else { return }
        artworkURL = url
        artwork = nil
        artworkTask?.cancel()
        guard let url, let address = URL(string: url) else { return }
        artworkTask = Task { [weak self] in
            guard let (data, _) = try? await URLSession.shared.data(from: address),
                  let image = NSImage(data: data),
                  !Task.isCancelled,
                  let self, self.artworkURL == url
            else { return }
            // The handler may run on any thread, so it makes its image from the data rather
            // than sharing this one.
            let artwork = MPMediaItemArtwork(boundsSize: image.size) { _ in
                NSImage(data: data) ?? NSImage()
            }
            self.artwork = artwork
            if var properties = self.info.nowPlayingInfo {
                properties[MPMediaItemPropertyArtwork] = artwork
                self.info.nowPlayingInfo = properties
            }
        }
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
