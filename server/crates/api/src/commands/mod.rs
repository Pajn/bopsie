//! The commands this server implements.

pub mod auth;
pub mod empty;

use crate::command::Registry;

pub fn registry() -> Registry {
    let mut registry = Registry::default();
    registry
        .add::<auth::Login>()
        .add::<auth::Me>()
        .add::<auth::Logout>()
        .add::<auth::CreateToken>()
        .add::<empty::ArtistItems>()
        .add::<empty::ArtistCount>()
        .add::<empty::AlbumItems>()
        .add::<empty::AlbumCount>()
        .add::<empty::TrackItems>()
        .add::<empty::TrackCount>()
        .add::<empty::PlaylistItems>()
        .add::<empty::PlaylistCount>()
        .add::<empty::Search>()
        .add::<empty::AllPlayers>()
        .add::<empty::GetQueue>();
    registry
}
