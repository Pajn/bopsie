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
        "
        -- Items that may be the same, for a person to decide, and what they decided: 'merge'
        -- or 'apart'. a is the lower id.
        CREATE TABLE match_suggestions (
            id INTEGER PRIMARY KEY,
            media_type TEXT NOT NULL,
            a INTEGER NOT NULL,
            b INTEGER NOT NULL,
            reason TEXT NOT NULL,
            decision TEXT,
            created INTEGER NOT NULL,
            UNIQUE (media_type, a, b)
        ) STRICT;
    ",
        "
        -- When a metadata service such as MusicBrainz was last asked about each album.
        ALTER TABLE albums ADD COLUMN metadata_checked INTEGER;

        -- A release an album looks like but was not certain enough to take, for a person to
        -- accept or reject: 'accept' or 'reject'.
        CREATE TABLE release_suggestions (
            album_id INTEGER PRIMARY KEY,
            release_id TEXT NOT NULL,
            title TEXT NOT NULL,
            distance REAL NOT NULL,
            decision TEXT,
            created INTEGER NOT NULL
        ) STRICT;
    ",
        "
        -- Genres, most voted first, as a metadata service such as MusicBrainz has them.
        CREATE TABLE item_genres (
            media_type TEXT NOT NULL,
            item_id INTEGER NOT NULL,
            provider_instance TEXT NOT NULL,
            position INTEGER NOT NULL,
            genre TEXT NOT NULL,
            PRIMARY KEY (media_type, item_id, provider_instance, position)
        ) STRICT;
    ",
        "
        -- Items merged into another, so that a queue, or a client, holding the id of one that
        -- is gone finds the item it became.
        CREATE TABLE merged_items (
            media_type TEXT NOT NULL,
            from_id INTEGER NOT NULL,
            into_id INTEGER NOT NULL,
            PRIMARY KEY (media_type, from_id)
        ) STRICT;
    ",
        "
        -- Albums a metadata service found nothing for are asked about again: tracks are now
        -- paired by song, not by place, so an edition in another order can be found.
        UPDATE albums SET metadata_checked = NULL
        WHERE id NOT IN (SELECT item_id FROM external_ids
                         WHERE media_type = 'album' AND provider_instance = 'musicbrainz')
            AND id NOT IN (SELECT album_id FROM release_suggestions);
    ",
        "
        -- Albums only suggested a release are asked about again: a track kept twice no longer
        -- counts as one the release lacks, so some are now certain enough to take.
        UPDATE albums SET metadata_checked = NULL
        WHERE id IN (SELECT album_id FROM release_suggestions WHERE decision IS NULL);
    ",
        "
        -- An item's images beyond its own artwork, by kind (thumb, fanart, logo, banner), as
        -- each source has them: an artist's photo, its backgrounds and its logo.
        CREATE TABLE item_images (
            media_type TEXT NOT NULL,
            item_id INTEGER NOT NULL,
            provider_instance TEXT NOT NULL,
            kind TEXT NOT NULL,
            position INTEGER NOT NULL,
            proxy_id TEXT NOT NULL,
            PRIMARY KEY (media_type, item_id, provider_instance, kind, position)
        ) STRICT;

        -- When each artist's images were last looked for.
        ALTER TABLE artists ADD COLUMN images_checked INTEGER;

        -- Spotify's tracks are read again once, for their artists' Spotify ids.
        UPDATE provider_mappings SET stamp = NULL
        WHERE media_type = 'track' AND provider_domain = 'spotify';
    ",
        "
        -- The provider's album each of its tracks came from, such as a folder: an album
        -- mapping no track came from is pruned, as a deleted copy of a folder is.
        ALTER TABLE provider_mappings ADD COLUMN provider_album TEXT;

        -- A file's album is its folder: the path up to its last slash.
        UPDATE provider_mappings
        SET provider_album = rtrim(rtrim(provider_item_id, replace(provider_item_id, '/', '')), '/')
        WHERE media_type = 'track' AND provider_domain = 'filesystem_local';
    ",
        "
        -- What a suggested release is, to compare with the album: its artist and title, year,
        -- country, formats (a JSON array, one per kind of medium) and how many tracks and discs.
        ALTER TABLE release_suggestions ADD COLUMN artist TEXT;
        ALTER TABLE release_suggestions ADD COLUMN release_title TEXT;
        ALTER TABLE release_suggestions ADD COLUMN year INTEGER;
        ALTER TABLE release_suggestions ADD COLUMN country TEXT;
        ALTER TABLE release_suggestions ADD COLUMN formats TEXT;
        ALTER TABLE release_suggestions ADD COLUMN track_count INTEGER;
        ALTER TABLE release_suggestions ADD COLUMN disc_count INTEGER;

        -- Albums with an undecided suggestion are asked about again, to have them.
        UPDATE albums SET metadata_checked = NULL
        WHERE id IN (SELECT album_id FROM release_suggestions WHERE decision IS NULL);
    ",
        "
        -- How loud each source's file is, as ReplayGain 2 gains (dB to -18 LUFS) and sample
        -- peaks: from its tags ('tags') or measured ('measured'). loudness_stamp is the stamp
        -- of the file they hold for; a file whose stamp differs is due to be measured.
        ALTER TABLE provider_mappings ADD COLUMN track_gain REAL;
        ALTER TABLE provider_mappings ADD COLUMN track_peak REAL;
        ALTER TABLE provider_mappings ADD COLUMN album_gain REAL;
        ALTER TABLE provider_mappings ADD COLUMN album_peak REAL;
        ALTER TABLE provider_mappings ADD COLUMN loudness TEXT;
        ALTER TABLE provider_mappings ADD COLUMN loudness_stamp TEXT;

        -- Files are read again once, for their loudness tags.
        UPDATE provider_mappings SET stamp = NULL
        WHERE media_type = 'track' AND provider_domain = 'filesystem_local';
    ",
        "
        -- How often each item was played, and when last, in Unix seconds: a track once heard,
        -- with its album and artists, and a playlist when it is played.
        CREATE TABLE plays (
            media_type TEXT NOT NULL,
            item_id INTEGER NOT NULL,
            play_count INTEGER NOT NULL,
            last_played INTEGER NOT NULL,
            PRIMARY KEY (media_type, item_id)
        ) STRICT, WITHOUT ROWID;

        CREATE TRIGGER artists_plays AFTER DELETE ON artists BEGIN
            DELETE FROM plays WHERE media_type = 'artist' AND item_id = old.id;
        END;
        CREATE TRIGGER albums_plays AFTER DELETE ON albums BEGIN
            DELETE FROM plays WHERE media_type = 'album' AND item_id = old.id;
        END;
        CREATE TRIGGER tracks_plays AFTER DELETE ON tracks BEGIN
            DELETE FROM plays WHERE media_type = 'track' AND item_id = old.id;
        END;
        CREATE TRIGGER playlists_plays AFTER DELETE ON playlists BEGIN
            DELETE FROM plays WHERE media_type = 'playlist' AND item_id = old.id;
        END;
    ",
    ],
};
