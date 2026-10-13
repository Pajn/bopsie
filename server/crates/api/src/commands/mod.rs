//! The commands this server implements.

pub mod auth;
pub mod empty;
pub mod music;

use crate::command::Registry;

pub fn registry() -> Registry {
    let mut registry = Registry::default();
    registry
        .add::<auth::Login>()
        .add::<auth::Me>()
        .add::<auth::Logout>()
        .add::<auth::CreateToken>()
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
        .add::<empty::AllPlayers>()
        .add::<empty::GetQueue>();
    registry
}
