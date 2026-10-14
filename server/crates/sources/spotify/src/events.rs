//! What librespot's player events mean for the source: its state, and what to tell the
//! groups.

use bopsie_sources::{Happened, SourceMedia, SourceState};
use librespot_metadata::audio::{AudioItem, UniqueFields};
use librespot_playback::player::PlayerEvent;

/// Applies `event`, at `now` on the server's clock, to `state`. Returns what the groups should
/// hear of it.
pub fn apply(state: &mut SourceState, event: &PlayerEvent, now: i64) -> Option<Happened> {
    let at = |state: &mut SourceState, position_ms: u32| {
        state.position_ms = u64::from(position_ms);
        state.at = now;
    };
    match event {
        // Each track that starts says it plays; only the first after a pause is news.
        PlayerEvent::Playing { position_ms, .. } => {
            let was = state.playing;
            state.playing = true;
            state.connected = true;
            at(state, *position_ms);
            (!was).then_some(Happened::Playing)
        }
        PlayerEvent::Paused { position_ms, .. } => {
            let was = state.playing;
            state.playing = false;
            at(state, *position_ms);
            was.then_some(Happened::Paused)
        }
        PlayerEvent::Stopped { .. } => {
            let was = state.playing;
            state.playing = false;
            state.position_ms = state.position_at(now);
            state.at = now;
            was.then_some(Happened::Paused)
        }
        PlayerEvent::Seeked { position_ms, .. } => {
            at(state, *position_ms);
            Some(Happened::Seeked)
        }
        PlayerEvent::PositionCorrection { position_ms, .. } => {
            at(state, *position_ms);
            None
        }
        PlayerEvent::TrackChanged { audio_item } => {
            state.media = Some(media(audio_item));
            at(state, 0);
            Some(Happened::MediaChanged)
        }
        PlayerEvent::SessionConnected { .. } => {
            state.connected = true;
            None
        }
        PlayerEvent::SessionDisconnected { .. } => {
            let was = state.connected;
            state.connected = false;
            state.playing = false;
            was.then_some(Happened::Ended)
        }
        _ => None,
    }
}

/// What a track or episode is, for the groups and the apps.
pub fn media(item: &AudioItem) -> SourceMedia {
    let (artist, album) = match &item.unique_fields {
        UniqueFields::Track { artists, album, .. } => (
            Some(
                artists
                    .iter()
                    .map(|artist| artist.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            )
            .filter(|names| !names.is_empty()),
            Some(album.clone()).filter(|album| !album.is_empty()),
        ),
        UniqueFields::Local { artists, album, .. } => (artists.clone(), album.clone()),
        UniqueFields::Episode { show_name, .. } => (Some(show_name.clone()), None),
    };
    SourceMedia {
        title: item.name.clone(),
        artist,
        album,
        artwork_url: item
            .covers
            .iter()
            .max_by_key(|cover| cover.width)
            .map(|cover| cover.url.clone()),
        duration_ms: u64::from(item.duration_ms),
        uri: Some(item.uri.clone()),
    }
}

#[cfg(test)]
mod tests {
    use librespot_core::SpotifyUri;

    use super::*;

    fn track() -> SpotifyUri {
        SpotifyUri::from_uri("spotify:track:3eDYzM3QrzbKGze39nyRYU").unwrap()
    }

    #[test]
    fn playing_pausing_and_seeking() {
        let mut state = SourceState::default();
        let playing = |position_ms| PlayerEvent::Playing {
            play_request_id: 1,
            track_id: track(),
            position_ms,
        };
        assert_eq!(
            apply(&mut state, &playing(0), 1_000_000),
            Some(Happened::Playing)
        );
        assert!(state.playing && state.connected);
        // The next track starting is no news.
        assert_eq!(apply(&mut state, &playing(0), 2_000_000), None);
        let paused = PlayerEvent::Paused {
            play_request_id: 1,
            track_id: track(),
            position_ms: 4_000,
        };
        assert_eq!(
            apply(&mut state, &paused, 6_000_000),
            Some(Happened::Paused)
        );
        assert_eq!(state.position_at(60_000_000), 4_000);
        assert_eq!(apply(&mut state, &paused, 7_000_000), None);
        let seeked = PlayerEvent::Seeked {
            play_request_id: 1,
            track_id: track(),
            position_ms: 1_000,
        };
        assert_eq!(
            apply(&mut state, &seeked, 8_000_000),
            Some(Happened::Seeked)
        );
        assert_eq!(state.position_ms, 1_000);
        let gone = PlayerEvent::SessionDisconnected {
            connection_id: String::new(),
            user_name: String::new(),
        };
        assert_eq!(apply(&mut state, &gone, 9_000_000), Some(Happened::Ended));
        assert!(!state.playing && !state.connected);
        assert_eq!(apply(&mut state, &gone, 9_000_000), None);
    }
}
