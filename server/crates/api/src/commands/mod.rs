//! The commands this server implements.

pub mod admin;
pub mod airplay;
pub mod auth;
pub mod calibration;
pub mod music;
pub mod players;
pub mod queues;
pub mod settings;
pub mod spotify;
pub mod sync;

use crate::command::Registry;

pub fn registry() -> Registry {
    let mut registry = Registry::default();
    registry
        .add::<auth::Login>()
        .add::<auth::Me>()
        .add::<auth::Logout>()
        .add::<auth::CreateToken>()
        .add::<auth::Users>()
        .add::<auth::GetUser>()
        .add::<auth::CreateUser>()
        .add::<auth::UpdateUser>()
        .add::<auth::EnableUser>()
        .add::<auth::DisableUser>()
        .add::<auth::DeleteUser>()
        .add::<auth::Tokens>()
        .add::<auth::RevokeToken>()
        .add::<music::ArtistItems>()
        .add::<music::ArtistCount>()
        .add::<music::AlbumItems>()
        .add::<music::AlbumCount>()
        .add::<music::TrackItems>()
        .add::<music::TrackCount>()
        .add::<music::PlaylistItems>()
        .add::<music::PlaylistCount>()
        .add::<music::GetArtist>()
        .add::<music::GetAlbum>()
        .add::<music::GetTrack>()
        .add::<music::GetPlaylist>()
        .add::<music::AlbumTracks>()
        .add::<music::ArtistAlbums>()
        .add::<music::ArtistTracks>()
        .add::<music::PlaylistTracks>()
        .add::<music::ItemByUri>()
        .add::<music::Search>()
        .add::<music::Sync>()
        .add::<music::Providers>()
        .add::<players::All>()
        .add::<players::Get>()
        .add::<players::VolumeSet>()
        .add::<players::VolumeMute>()
        .add::<players::GroupVolume>()
        .add::<players::Group>()
        .add::<players::Ungroup>()
        .add::<queues::Get>()
        .add::<queues::GetActiveQueue>()
        .add::<queues::Items>()
        .add::<queues::PlayMedia>()
        .add::<queues::PlayIndex>()
        .add::<queues::Play>()
        .add::<queues::Pause>()
        .add::<queues::PlayPause>()
        .add::<queues::Stop>()
        .add::<queues::Next>()
        .add::<queues::Previous>()
        .add::<queues::Clear>()
        .add::<queues::Seek>()
        .add::<queues::Skip>()
        .add::<queues::Shuffle>()
        .add::<queues::Repeat>()
        .add::<queues::MoveItem>()
        .add::<queues::DeleteItem>()
        .add::<queues::Transfer>()
        .add::<airplay::PairStart>()
        .add::<airplay::PairFinish>()
        .add::<admin::Devices>()
        .add::<admin::Groups>()
        .add::<admin::Server>()
        .add::<admin::Logs>()
        .add::<settings::PlayerUpdate>()
        .add::<settings::EndpointUpdate>()
        .add::<settings::LibraryUpdate>()
        .add::<settings::LibraryEdits>()
        .add::<settings::LibrarySuggestions>()
        .add::<settings::LibraryDecide>()
        .add::<settings::LibrarySeparate>()
        .add::<settings::ReleaseSuggestions>()
        .add::<settings::DecideRelease>()
        .add::<settings::RefreshMetadata>()
        .add::<settings::Reconsider>()
        .add::<settings::ReconsiderRelease>()
        .add::<settings::DuplicateFiles>()
        .add::<calibration::CalibrationStart>()
        .add::<calibration::CalibrationApply>()
        .add::<sync::Sync>()
        .add::<sync::SyncUpdate>()
        .add::<spotify::Status>()
        .add::<spotify::SetClientId>()
        .add::<spotify::Login>()
        .add::<spotify::PairStart>()
        .add::<spotify::PairCancel>()
        .add::<spotify::SignOut>();
    registry
}
