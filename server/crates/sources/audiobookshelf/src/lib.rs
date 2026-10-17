//! Audiobookshelf as a source of audiobooks: signed in to from the web UI, browsed and searched
//! as the server has its books, played through the server, and with listening progress kept
//! on the server, so a book carries on in Audiobookshelf's own apps where it left off here.

pub mod account;
pub mod catalog;

pub use account::{Account, AccountError, Status};
pub use catalog::{AUDIOBOOKSHELF, AudiobookshelfCatalog};
