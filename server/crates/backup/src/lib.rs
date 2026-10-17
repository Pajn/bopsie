//! Backups of the whole server: its database and the rest of its data directory, as one
//! zstd-compressed tar archive, encrypted with a passphrase in the age format when one is given.
//!
//! A backup is made while the server runs: the database is copied with `VACUUM INTO`, which
//! sees one consistent state, and the data directory's other entries (artwork, pairings, logins)
//! are read as they are. Restoring one happens in two halves. [`stage`] unpacks and checks it
//! beside the data directory while the server runs; [`apply_staged`], at the next start and
//! before the database is opened, moves it into place and keeps what it replaces.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use age::secrecy::SecretString;
use bopsie_core::db::{Database, DbError, Migrations};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::macros::format_description;

/// What a backup says of itself, as `bopsie-backup.json` at the head of the archive.
pub const MANIFEST: &str = "bopsie-backup.json";
/// The archive's layout; a server reads only the layouts it knows.
pub const FORMAT: u32 = 1;
/// The database's file name, in the data directory and in a backup.
pub const DATABASE: &str = "bopsie.db";
/// Where a backup being restored is unpacked, in the data directory.
pub const RESTORE: &str = "restore";
/// What a restore keeps of the state it replaced, in the data directory, then a timestamp.
pub const BEFORE_RESTORE: &str = "before-restore-";
/// Written last when a staged backup is complete and checked.
const READY: &str = "READY";
/// How hard zstd compresses: its default, quick, and most of a backup is images anyway.
const LEVEL: i32 = 3;
/// How an encrypted archive begins: the age format's header.
const AGE_HEADER: &[u8] = b"age-encryption.org/v1";

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("the backup is encrypted; its passphrase is needed")]
    PassphraseNeeded,
    #[error("the passphrase does not open the backup")]
    WrongPassphrase,
    #[error("not a Bopsie backup: {0}")]
    NotABackup(String),
    #[error(
        "the backup is from a newer server: its {component} schema is version {found}, but this server knows only {known}"
    )]
    TooNew {
        component: String,
        found: u32,
        known: u32,
    },
    #[error("no backup is named {0}")]
    NoSuchBackup(String),
}

impl From<rusqlite::Error> for BackupError {
    fn from(error: rusqlite::Error) -> Self {
        BackupError::Db(DbError::from(error))
    }
}

pub type Result<T> = std::result::Result<T, BackupError>;

/// What a backup says of itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// Unix seconds.
    pub created: i64,
    pub server_version: String,
    /// Each database component's schema version.
    pub schema: BTreeMap<String, u32>,
}

/// A backup kept in the backups directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Backup {
    pub name: String,
    /// Bytes.
    pub size: u64,
    /// Unix seconds.
    pub created: i64,
    pub encrypted: bool,
    /// Made on the schedule, and pruned by it.
    pub scheduled: bool,
}

/// The file name of a backup made at `created`.
pub fn name_for(created: OffsetDateTime, encrypted: bool, scheduled: bool) -> String {
    let stamp = created
        .to_offset(time::UtcOffset::UTC)
        .format(format_description!(
            "[year][month][day]-[hour][minute][second]"
        ))
        .expect("a fixed format");
    format!(
        "bopsie-{}{stamp}.tar.zst{}",
        if scheduled { "scheduled-" } else { "" },
        if encrypted { ".age" } else { "" }
    )
}

/// A backup's name read back: when it was made, whether it is encrypted and scheduled. `None`
/// for anything else, so no name leads out of the backups directory.
pub fn parse_name(name: &str) -> Option<(i64, bool, bool)> {
    let rest = name.strip_prefix("bopsie-")?;
    let (scheduled, rest) = match rest.strip_prefix("scheduled-") {
        Some(rest) => (true, rest),
        None => (false, rest),
    };
    let (stamp, encrypted) = match rest.strip_suffix(".tar.zst.age") {
        Some(stamp) => (stamp, true),
        None => (rest.strip_suffix(".tar.zst")?, false),
    };
    if stamp.len() != 15 || !stamp.chars().all(|c| c.is_ascii_digit() || c == '-') {
        return None;
    }
    let created = time::PrimitiveDateTime::parse(
        stamp,
        format_description!("[year][month][day]-[hour][minute][second]"),
    )
    .ok()?
    .assume_utc()
    .unix_timestamp();
    Some((created, encrypted, scheduled))
}

/// Each component's schema version in the database at `path`.
fn schema_of(path: &Path) -> Result<BTreeMap<String, u32>> {
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut statement = conn.prepare("SELECT component, version FROM schema_migrations")?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Whether the data directory's entry `name` is backed up: not the database, which is copied
/// apart, nor a restore's leftovers.
fn backed_up(name: &str) -> bool {
    !name.starts_with(DATABASE) && name != RESTORE && !name.starts_with(BEFORE_RESTORE)
}

/// Whether `path` is `dir` itself, as a backups directory inside the data directory is.
fn same_place(path: &Path, dir: &Path) -> bool {
    match (path.canonicalize(), dir.canonicalize()) {
        (Ok(path), Ok(dir)) => path == dir,
        _ => false,
    }
}

/// Makes a backup of `db` and `data_dir` in `dir`, encrypted with `passphrase` when there is
/// one. It is written under a temporary name and renamed when whole, so a backup listed is a
/// complete one.
pub async fn create(
    db: &Database,
    data_dir: &Path,
    dir: &Path,
    passphrase: Option<&str>,
    scheduled: bool,
    server_version: &str,
) -> Result<Backup> {
    fs::create_dir_all(dir)?;
    let now = OffsetDateTime::now_utc();
    let name = name_for(now, passphrase.is_some(), scheduled);
    let snapshot = dir.join(format!(".{name}.db"));
    let _ = fs::remove_file(&snapshot);
    let target = snapshot.to_string_lossy().into_owned();
    db.call(move |conn| {
        conn.execute("VACUUM INTO ?1", [target])?;
        Ok(())
    })
    .await?;
    let manifest = Manifest {
        format: FORMAT,
        created: now.unix_timestamp(),
        server_version: server_version.to_owned(),
        schema: schema_of(&snapshot)?,
    };
    let path = dir.join(&name);
    let partial = dir.join(format!(".{name}.partial"));
    let (data_dir, dir, passphrase, final_path) = (
        data_dir.to_owned(),
        dir.to_owned(),
        passphrase.map(str::to_owned),
        path.clone(),
    );
    let result = tokio::task::spawn_blocking(move || {
        let written = write_archive(&partial, &manifest, &snapshot, &data_dir, &dir, passphrase)
            .and_then(|()| Ok(fs::rename(&partial, &final_path)?));
        let _ = fs::remove_file(&snapshot);
        if written.is_err() {
            let _ = fs::remove_file(&partial);
        }
        written
    })
    .await
    .map_err(|error| io::Error::other(error.to_string()))?;
    result?;
    Ok(Backup {
        size: fs::metadata(&path)?.len(),
        created: now.unix_timestamp(),
        encrypted: name.ends_with(".age"),
        scheduled,
        name,
    })
}

fn write_archive(
    path: &Path,
    manifest: &Manifest,
    snapshot: &Path,
    data_dir: &Path,
    backups: &Path,
    passphrase: Option<String>,
) -> Result<()> {
    let mut file = BufWriter::new(File::create(path)?);
    match passphrase {
        Some(passphrase) => {
            let encryptor = age::Encryptor::with_user_passphrase(SecretString::from(passphrase));
            let mut encrypted = encryptor
                .wrap_output(&mut file)
                .map_err(|error| io::Error::other(error.to_string()))?;
            write_tar(&mut encrypted, manifest, snapshot, data_dir, backups)?;
            encrypted.finish()?;
        }
        None => write_tar(&mut file, manifest, snapshot, data_dir, backups)?,
    }
    file.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    Ok(())
}

fn write_tar(
    out: &mut impl Write,
    manifest: &Manifest,
    snapshot: &Path,
    data_dir: &Path,
    backups: &Path,
) -> Result<()> {
    let compressed = zstd::Encoder::new(out, LEVEL)?;
    let mut tar = tar::Builder::new(compressed);
    tar.follow_symlinks(false);
    let json = serde_json::to_vec_pretty(manifest).map_err(io::Error::other)?;
    let mut header = tar::Header::new_gnu();
    header.set_size(json.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(manifest.created.max(0) as u64);
    header.set_cksum();
    tar.append_data(&mut header, MANIFEST, json.as_slice())?;
    tar.append_path_with_name(snapshot, DATABASE)?;
    let mut entries: Vec<_> = fs::read_dir(data_dir)?.collect::<io::Result<_>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let path = entry.path();
        if !backed_up(name) || same_place(&path, backups) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            tar.append_dir_all(name, &path)?;
        } else {
            tar.append_path_with_name(&path, name)?;
        }
    }
    tar.into_inner()?.finish()?;
    Ok(())
}

/// The backups in `dir`, newest first.
pub fn list(dir: &Path) -> Result<Vec<Backup>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut backups = Vec::new();
    for entry in entries {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some((created, encrypted, scheduled)) = parse_name(&name) else {
            continue;
        };
        backups.push(Backup {
            size: entry.metadata()?.len(),
            name,
            created,
            encrypted,
            scheduled,
        });
    }
    backups.sort_by(|a, b| b.created.cmp(&a.created).then(b.name.cmp(&a.name)));
    Ok(backups)
}

/// Where backup `name` is in `dir`, when it is one there.
pub fn path_of(dir: &Path, name: &str) -> Result<PathBuf> {
    let path = dir.join(name);
    if parse_name(name).is_none() || !path.is_file() {
        return Err(BackupError::NoSuchBackup(name.to_owned()));
    }
    Ok(path)
}

pub fn delete(dir: &Path, name: &str) -> Result<()> {
    Ok(fs::remove_file(path_of(dir, name)?)?)
}

/// Removes the scheduled backups in `dir` but the newest `keep`; the others are left alone.
/// Answers with the names removed.
pub fn prune(dir: &Path, keep: usize) -> Result<Vec<String>> {
    let mut removed = Vec::new();
    for backup in list(dir)?
        .into_iter()
        .filter(|backup| backup.scheduled)
        .skip(keep)
    {
        delete(dir, &backup.name)?;
        removed.push(backup.name);
    }
    Ok(removed)
}

/// The archive at `path`, decrypted when it is encrypted, as its tar stream.
fn open(path: &Path, passphrase: Option<&str>) -> Result<Box<dyn Read>> {
    let mut file = BufReader::new(File::open(path)?);
    let mut head = [0u8; AGE_HEADER.len()];
    let read = read_up_to(&mut file, &mut head)?;
    let encrypted = &head[..read] == AGE_HEADER;
    let file = io::Cursor::new(head[..read].to_vec()).chain(file);
    let reader: Box<dyn Read> = if encrypted {
        let passphrase = passphrase.ok_or(BackupError::PassphraseNeeded)?;
        let decryptor = age::Decryptor::new(BufReader::new(file))
            .map_err(|error| BackupError::NotABackup(error.to_string()))?;
        let identity = age::scrypt::Identity::new(SecretString::from(passphrase.to_owned()));
        let decrypted = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .map_err(|_| BackupError::WrongPassphrase)?;
        Box::new(decrypted)
    } else {
        Box::new(file)
    };
    let decoder =
        zstd::Decoder::new(reader).map_err(|error| BackupError::NotABackup(error.to_string()))?;
    Ok(Box::new(decoder))
}

fn read_up_to(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// Unpacks the backup at `archive` into the data directory's `restore/`, and checks it: a
/// backup of a layout this server reads, whose database it can open. `known` is the schema
/// this server brings. Once checked, it is restored at the next start.
pub fn stage(
    archive: &Path,
    passphrase: Option<&str>,
    data_dir: &Path,
    known: &[Migrations],
) -> Result<Manifest> {
    let staging = data_dir.join(RESTORE);
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;
    let staged = unpack(archive, passphrase, &staging, known);
    if staged.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    staged
}

fn unpack(
    archive: &Path,
    passphrase: Option<&str>,
    staging: &Path,
    known: &[Migrations],
) -> Result<Manifest> {
    let reader = open(archive, passphrase)?;
    tar::Archive::new(reader)
        .unpack(staging)
        .map_err(|error| BackupError::NotABackup(error.to_string()))?;
    let manifest = fs::read(staging.join(MANIFEST))
        .map_err(|_| BackupError::NotABackup(format!("it holds no {MANIFEST}")))?;
    let manifest: Manifest = serde_json::from_slice(&manifest)
        .map_err(|error| BackupError::NotABackup(error.to_string()))?;
    if manifest.format != FORMAT {
        return Err(BackupError::NotABackup(format!(
            "its layout is version {}, and this server reads {FORMAT}",
            manifest.format
        )));
    }
    let database = staging.join(DATABASE);
    if !database.is_file() {
        return Err(BackupError::NotABackup(format!("it holds no {DATABASE}")));
    }
    // The database itself says what it is.
    for (component, found) in schema_of(&database)? {
        let Some(set) = known.iter().find(|set| set.component == component) else {
            continue;
        };
        let known = set.steps.len() as u32;
        if found > known {
            return Err(BackupError::TooNew {
                component,
                found,
                known,
            });
        }
    }
    fs::write(staging.join(READY), b"")?;
    Ok(manifest)
}

/// Restores the backup [`stage`] left in the data directory, if there is one: what is there
/// now moves to `before-restore-<time>/`, and the backup takes its place. `keep` are entries of
/// the data directory left where they are, such as the backups directory inside it. Run before
/// the database is opened. Answers with the backup's manifest.
pub fn apply_staged(data_dir: &Path, keep: &[&Path]) -> Result<Option<Manifest>> {
    let staging = data_dir.join(RESTORE);
    if !staging.join(READY).is_file() {
        return Ok(None);
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(staging.join(MANIFEST))?)
        .map_err(|error| BackupError::NotABackup(error.to_string()))?;
    let stamp = OffsetDateTime::now_utc().unix_timestamp();
    let before = data_dir.join(format!("{BEFORE_RESTORE}{stamp}"));
    fs::create_dir_all(&before)?;
    for entry in fs::read_dir(data_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let path = entry.path();
        if name == RESTORE
            || name.starts_with(BEFORE_RESTORE)
            || keep.iter().any(|kept| same_place(&path, kept))
        {
            continue;
        }
        fs::rename(&path, before.join(name))?;
    }
    for entry in fs::read_dir(&staging)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == READY || name == MANIFEST {
            continue;
        }
        fs::rename(entry.path(), data_dir.join(name))?;
    }
    fs::remove_dir_all(&staging)?;
    Ok(Some(manifest))
}
