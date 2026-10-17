//! Making a backup of a data directory and restoring it into another.

use std::fs;
use std::path::Path;

use bopsie_backup::{BackupError, apply_staged, create, delete, list, parse_name, prune, stage};
use bopsie_core::db::{Database, Migrations, set_setting, setting};

const NOTES: Migrations = Migrations {
    component: "notes",
    steps: &["CREATE TABLE notes (text TEXT NOT NULL) STRICT;"],
};

/// A data directory as a server keeps it: its database, artwork, pairings, and a backups
/// directory inside it.
fn data_dir(root: &Path) -> Database {
    fs::create_dir_all(root.join("artwork")).unwrap();
    fs::write(root.join("artwork/cover.jpg"), b"jpeg").unwrap();
    fs::create_dir_all(root.join("airplay")).unwrap();
    fs::write(root.join("airplay/keys"), b"pairing").unwrap();
    // Leftovers of an earlier restore, which a backup leaves out.
    fs::create_dir_all(root.join("before-restore-1")).unwrap();
    fs::write(root.join("before-restore-1/old"), b"old").unwrap();
    Database::open(&root.join("bopsie.db"), &[NOTES]).unwrap()
}

async fn note(db: &Database, text: &'static str) {
    db.call(move |conn| {
        conn.execute("INSERT INTO notes (text) VALUES (?1)", [text])?;
        set_setting(conn, "last", Some(text))?;
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_backup_restores_the_database_and_the_files() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let backups = data.join("backups");
    let db = data_dir(&data);
    note(&db, "kept").await;

    let backup = create(&db, &data, &backups, None, false, "1.2.3")
        .await
        .unwrap();
    assert!(!backup.encrypted && !backup.scheduled);
    assert!(backup.name.ends_with(".tar.zst"), "{}", backup.name);
    assert_eq!(list(&backups).unwrap(), std::slice::from_ref(&backup));
    // Only the finished backup is left in the directory.
    assert_eq!(fs::read_dir(&backups).unwrap().count(), 1);

    // Changed since; then restored.
    note(&db, "after").await;
    fs::write(data.join("artwork/cover.jpg"), b"changed").unwrap();
    drop(db);
    let manifest = stage(&backups.join(&backup.name), None, &data, &[NOTES]).unwrap();
    assert_eq!(manifest.server_version, "1.2.3");
    assert_eq!(manifest.schema["notes"], 1);
    assert!(data.join("restore").is_dir());
    let applied = apply_staged(&data, &[&backups]).unwrap();
    assert_eq!(applied, Some(manifest));
    assert!(!data.join("restore").exists());

    let db = Database::open(&data.join("bopsie.db"), &[NOTES]).unwrap();
    let (notes, last) = db
        .call(|conn| {
            let notes: u32 = conn.query_row("SELECT count(*) FROM notes", [], |r| r.get(0))?;
            Ok((notes, setting(conn, "last")?))
        })
        .await
        .unwrap();
    assert_eq!((notes, last.as_deref()), (1, Some("kept")));
    assert_eq!(fs::read(data.join("artwork/cover.jpg")).unwrap(), b"jpeg");
    assert_eq!(fs::read(data.join("airplay/keys")).unwrap(), b"pairing");
    // What it replaced is kept, and the backups stay where they are.
    let before = fs::read_dir(&data)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .find(|name| name.starts_with("before-restore-") && name != "before-restore-1")
        .expect("the state it replaced is kept");
    assert_eq!(
        fs::read(data.join(&before).join("artwork/cover.jpg")).unwrap(),
        b"changed"
    );
    assert_eq!(list(&backups).unwrap().len(), 1);
    // Nothing staged, nothing to do.
    assert_eq!(apply_staged(&data, &[&backups]).unwrap(), None);
}

#[tokio::test]
async fn a_backup_leaves_out_the_backups_and_earlier_restores() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let backups = data.join("backups");
    let db = data_dir(&data);
    let first = create(&db, &data, &backups, None, false, "1")
        .await
        .unwrap();
    let staging = dir.path().join("other");
    fs::create_dir_all(&staging).unwrap();
    stage(&backups.join(&first.name), None, &staging, &[NOTES]).unwrap();
    let mut held: Vec<String> = fs::read_dir(staging.join("restore"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    held.sort();
    assert_eq!(
        held,
        [
            "READY",
            "airplay",
            "artwork",
            "bopsie-backup.json",
            "bopsie.db"
        ]
    );
}

#[tokio::test]
async fn an_encrypted_backup_opens_only_with_its_passphrase() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let backups = dir.path().join("elsewhere");
    let db = data_dir(&data);
    let backup = create(&db, &data, &backups, Some("correct horse"), true, "1")
        .await
        .unwrap();
    assert!(backup.encrypted && backup.scheduled);
    assert!(backup.name.ends_with(".tar.zst.age"), "{}", backup.name);
    let archive = backups.join(&backup.name);
    // Not readable as it is.
    let bytes = fs::read(&archive).unwrap();
    assert!(bytes.starts_with(b"age-encryption.org/v1"));
    assert!(!bytes.windows(7).any(|w| w == b"pairing"));

    assert!(matches!(
        stage(&archive, None, &data, &[NOTES]),
        Err(BackupError::PassphraseNeeded)
    ));
    assert!(matches!(
        stage(&archive, Some("wrong"), &data, &[NOTES]),
        Err(BackupError::WrongPassphrase)
    ));
    assert!(!data.join("restore").exists(), "nothing half staged");
    stage(&archive, Some("correct horse"), &data, &[NOTES]).unwrap();
    assert!(data.join("restore/READY").is_file());
}

#[tokio::test]
async fn a_backup_of_a_newer_server_or_anything_else_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let backups = dir.path().join("backups");
    let db = data_dir(&data);
    let backup = create(&db, &data, &backups, None, false, "2")
        .await
        .unwrap();
    // This server knows the notes component before its first step.
    const OLDER: Migrations = Migrations {
        component: "notes",
        steps: &[],
    };
    let refused = stage(&backups.join(&backup.name), None, &data, &[OLDER]);
    assert!(
        matches!(&refused, Err(BackupError::TooNew { component, found: 1, known: 0 }) if component == "notes"),
        "{refused:?}"
    );
    assert!(!data.join("restore").exists());

    let junk = dir.path().join("junk.tar.zst");
    fs::write(&junk, b"not an archive at all").unwrap();
    assert!(matches!(
        stage(&junk, None, &data, &[NOTES]),
        Err(BackupError::NotABackup(_))
    ));
}

#[test]
fn names_say_when_and_how_and_lead_nowhere_else() {
    assert_eq!(
        parse_name("bopsie-20261017-181500.tar.zst"),
        Some((1_792_260_900, false, false))
    );
    assert_eq!(
        parse_name("bopsie-scheduled-20261017-181500.tar.zst.age"),
        Some((1_792_260_900, true, true))
    );
    for bad in [
        "../bopsie-20261017-181500.tar.zst",
        "bopsie-20261017-18150.tar.zst",
        "bopsie-2026101x-181500.tar.zst",
        "bopsie.db",
        "bopsie-20261017-181500.tar.gz",
    ] {
        assert_eq!(parse_name(bad), None, "{bad}");
    }
}

#[test]
fn pruning_keeps_the_newest_scheduled_backups_and_every_other() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "bopsie-scheduled-20261010-030000.tar.zst",
        "bopsie-scheduled-20261011-030000.tar.zst.age",
        "bopsie-scheduled-20261012-030000.tar.zst",
        "bopsie-20261001-120000.tar.zst",
        "notes.txt",
    ] {
        fs::write(dir.path().join(name), b"x").unwrap();
    }
    let removed = prune(dir.path(), 2).unwrap();
    assert_eq!(removed, ["bopsie-scheduled-20261010-030000.tar.zst"]);
    let names: Vec<String> = list(dir.path())
        .unwrap()
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(
        names,
        [
            "bopsie-scheduled-20261012-030000.tar.zst",
            "bopsie-scheduled-20261011-030000.tar.zst.age",
            "bopsie-20261001-120000.tar.zst",
        ]
    );
    assert!(dir.path().join("notes.txt").exists());
    assert!(matches!(
        delete(dir.path(), "notes.txt"),
        Err(BackupError::NoSuchBackup(_))
    ));
}
