//! What metadata services say of the library's albums, looked up in the background after each
//! sync: [`musicbrainz`] finds each album's release, [`score`] says how close a release is, and
//! [`job`] keeps what was found, or suggests it to a person when it is close but not certain.

pub mod job;
pub mod musicbrainz;
pub mod score;

use bopsie_core::db::Migrations;

pub const MIGRATIONS: Migrations = Migrations {
    component: "metadata",
    steps: &["
        -- Answers from metadata services, by URL, so that an album asked about again, or by
        -- another job, costs no request while they are fresh.
        CREATE TABLE metadata_cache (
            url TEXT PRIMARY KEY,
            body TEXT NOT NULL,
            fetched INTEGER NOT NULL
        ) STRICT;
    "],
};
