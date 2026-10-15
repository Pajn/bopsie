use bopsie_core::db::Migrations;

/// Items have integer ids, exposed as `library://<type>/<id>`. Artists and albums are matched
/// on a normalized key so that tracks from many files and providers meet in one item.
/// `provider_mappings` records where each item comes from; for tracks and playlists its
/// `stamp` is the scan journal. Search uses two FTS5 indexes over item names: trigrams for
/// substrings, words for short queries and prefixes.
pub const MIGRATIONS: Migrations = Migrations {
    component: "library",
    steps: &[
        "
        CREATE TABLE images (
            proxy_id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            path TEXT NOT NULL
        ) STRICT;

        CREATE TABLE artists (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            sort_name TEXT NOT NULL,
            match_key TEXT NOT NULL UNIQUE,
            favorite INTEGER NOT NULL DEFAULT 0,
            date_added INTEGER NOT NULL
        ) STRICT;

        CREATE TABLE albums (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            sort_name TEXT NOT NULL,
            match_key TEXT NOT NULL UNIQUE,
            year INTEGER,
            image TEXT REFERENCES images,
            favorite INTEGER NOT NULL DEFAULT 0,
            date_added INTEGER NOT NULL
        ) STRICT;

        CREATE TABLE album_artists (
            album_id INTEGER NOT NULL REFERENCES albums ON DELETE CASCADE,
            position INTEGER NOT NULL,
            artist_id INTEGER NOT NULL REFERENCES artists ON DELETE CASCADE,
            PRIMARY KEY (album_id, position)
        ) STRICT;
        CREATE INDEX album_artists_artist ON album_artists (artist_id);

        CREATE TABLE tracks (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            sort_name TEXT NOT NULL,
            duration INTEGER NOT NULL,
            album_id INTEGER REFERENCES albums ON DELETE SET NULL,
            disc_number INTEGER,
            track_number INTEGER,
            image TEXT REFERENCES images,
            favorite INTEGER NOT NULL DEFAULT 0,
            date_added INTEGER NOT NULL
        ) STRICT;
        CREATE INDEX tracks_album ON tracks (album_id);

        CREATE TABLE track_artists (
            track_id INTEGER NOT NULL REFERENCES tracks ON DELETE CASCADE,
            position INTEGER NOT NULL,
            artist_id INTEGER NOT NULL REFERENCES artists ON DELETE CASCADE,
            PRIMARY KEY (track_id, position)
        ) STRICT;
        CREATE INDEX track_artists_artist ON track_artists (artist_id);

        CREATE TABLE playlists (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            sort_name TEXT NOT NULL,
            favorite INTEGER NOT NULL DEFAULT 0,
            date_added INTEGER NOT NULL
        ) STRICT;

        CREATE TABLE playlist_tracks (
            playlist_id INTEGER NOT NULL REFERENCES playlists ON DELETE CASCADE,
            position INTEGER NOT NULL,
            track_id INTEGER NOT NULL REFERENCES tracks ON DELETE CASCADE,
            PRIMARY KEY (playlist_id, position)
        ) STRICT;

        CREATE TABLE provider_mappings (
            media_type TEXT NOT NULL,
            item_id INTEGER NOT NULL,
            provider_domain TEXT NOT NULL,
            provider_instance TEXT NOT NULL,
            provider_item_id TEXT NOT NULL,
            stamp TEXT,
            audio_format TEXT,
            PRIMARY KEY (provider_instance, media_type, provider_item_id)
        ) STRICT;
        CREATE INDEX provider_mappings_item ON provider_mappings (media_type, item_id);

        CREATE VIRTUAL TABLE search_trigram USING fts5(
            media_type UNINDEXED, item_id UNINDEXED, name,
            tokenize = 'trigram remove_diacritics 1'
        );
        CREATE VIRTUAL TABLE search_words USING fts5(
            media_type UNINDEXED, item_id UNINDEXED, name,
            tokenize = 'unicode61 remove_diacritics 2'
        );
    ",
        "
        -- Metadata edited by hand, as JSON: each edited field's value, and for albums and
        -- artists what it was scanned as. A sync applies them again over what it scanned.
        CREATE TABLE metadata_edits (
            media_type TEXT NOT NULL,
            item_id INTEGER NOT NULL,
            edits TEXT NOT NULL,
            scanned TEXT NOT NULL,
            PRIMARY KEY (media_type, item_id)
        ) STRICT;
    ",
        "
        -- A playlist's artwork, by proxy id, as albums have theirs.
        ALTER TABLE playlists ADD COLUMN image TEXT;
    ",
        "
        -- What identifies an item beyond its name, as each provider reports it: ISRCs,
        -- barcodes, MusicBrainz ids, a streaming service's ids. Ids go with their provider.
        CREATE TABLE external_ids (
            media_type TEXT NOT NULL,
            item_id INTEGER NOT NULL,
            provider_instance TEXT NOT NULL,
            kind TEXT NOT NULL,
            value TEXT NOT NULL,
            PRIMARY KEY (media_type, item_id, provider_instance, kind, value)
        ) STRICT;
        CREATE INDEX external_ids_value ON external_ids (kind, value);

        -- Names as candidates for being the same item are found by (ids::search_key).
        ALTER TABLE artists ADD COLUMN search_key TEXT;
        ALTER TABLE albums ADD COLUMN search_key TEXT;
        ALTER TABLE tracks ADD COLUMN search_key TEXT;
        ALTER TABLE playlists ADD COLUMN search_key TEXT;
        CREATE INDEX artists_search_key ON artists (search_key);
        CREATE INDEX albums_search_key ON albums (search_key);
        CREATE INDEX tracks_search_key ON tracks (search_key);

        -- Every track is read again once, for the ids it carries.
        UPDATE provider_mappings SET stamp = NULL WHERE media_type = 'track';
    ",
        "
        -- Which provider's copy of an item comes first, for its metadata and for playing:
        -- files before a streaming service's.
        ALTER TABLE provider_mappings ADD COLUMN priority INTEGER NOT NULL DEFAULT 0;
        UPDATE provider_mappings SET priority = 2 WHERE provider_domain = 'filesystem_local';
    ",
    ],
};
