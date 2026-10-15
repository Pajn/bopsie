//! The server's one SQLite database.
//!
//! A dedicated thread owns the connection and runs closures sent to it, so SQLite never blocks
//! the async executor. Each crate owns its tables and contributes its own migrations under a
//! component name; `schema_migrations` records how far each component has got.
//!
//! As every read and write waits its turn on that thread, slow work holds everything up. A
//! statement slower than [`SLOW_STATEMENT`] is logged with its SQL, and work that runs longer
//! than [`SLOW_JOB`], or waits longer than [`SLOW_WAIT`] for its turn, with where it came from.

use std::future::Future;
use std::panic::Location;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rusqlite::trace::{TraceEvent, TraceEventCodes};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::oneshot;

use crate::error::Error;

/// The ordered schema steps of one component. Steps are append-only: a released step never
/// changes, a later step amends it.
#[derive(Debug, Clone, Copy)]
pub struct Migrations {
    pub component: &'static str,
    pub steps: &'static [&'static str],
}

/// Server-wide values that are generated once and must survive restarts.
const CORE: Migrations = Migrations {
    component: "core",
    steps: &["CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;"],
};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(
        "the database has {component} schema version {found}, but this server knows only {known}"
    )]
    TooNew {
        component: &'static str,
        found: u32,
        known: u32,
    },
    #[error("the database thread has stopped")]
    Closed,
}

impl From<DbError> for Error {
    fn from(error: DbError) -> Self {
        tracing::error!(%error, "database error");
        Error::unexpected()
    }
}

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// A statement this slow is logged, with its SQL.
pub const SLOW_STATEMENT: Duration = Duration::from_millis(250);
/// Work on the database thread this slow is logged, with where it came from.
pub const SLOW_JOB: Duration = Duration::from_secs(2);
/// Work that waits this long for its turn is logged: something before it held the thread.
pub const SLOW_WAIT: Duration = Duration::from_secs(1);

/// `sql` on one line, at most a few hundred characters: enough to find it in the code.
fn one_line(sql: &str) -> String {
    let line = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(300) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    }
}

fn profile(event: TraceEvent<'_>) {
    if let TraceEvent::Profile(statement, took) = event
        && took >= SLOW_STATEMENT
    {
        tracing::warn!(
            ms = took.as_millis() as u64,
            sql = one_line(&statement.sql()),
            "slow database statement"
        );
    }
}

/// A handle to the database thread. Clones share the thread, which ends when the last handle
/// is dropped.
#[derive(Debug, Clone)]
pub struct Database {
    jobs: mpsc::Sender<Job>,
}

impl Database {
    /// Opens or creates the database file and brings every component's schema up to date.
    pub fn open(path: &Path, migrations: &[Migrations]) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::start(conn, migrations)
    }

    /// A private in-memory database, for tests.
    pub fn open_in_memory(migrations: &[Migrations]) -> Result<Self, DbError> {
        Self::start(Connection::open_in_memory()?, migrations)
    }

    fn start(mut conn: Connection, migrations: &[Migrations]) -> Result<Self, DbError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.trace_v2(TraceEventCodes::SQLITE_TRACE_PROFILE, Some(profile));
        migrate(&mut conn, &[&[CORE], migrations].concat())?;

        let (jobs, queue) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("sqlite".to_owned())
            .spawn(move || {
                for job in queue {
                    job(&mut conn);
                }
            })
            .expect("spawning the database thread");
        Ok(Self { jobs })
    }

    /// Runs `f` on the database thread and returns its result.
    #[track_caller]
    pub fn call<T, F>(&self, f: F) -> impl Future<Output = Result<T, DbError>> + use<T, F>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, DbError> + Send + 'static,
    {
        let caller = Location::caller();
        let queued = Instant::now();
        let (reply, result) = oneshot::channel();
        let sent = self
            .jobs
            .send(Box::new(move |conn| {
                let started = Instant::now();
                let waited = started - queued;
                if waited >= SLOW_WAIT {
                    tracing::warn!(
                        ms = waited.as_millis() as u64,
                        %caller,
                        "waited for the database"
                    );
                }
                let _ = reply.send(f(conn));
                let took = started.elapsed();
                if took >= SLOW_JOB {
                    tracing::warn!(ms = took.as_millis() as u64, %caller, "slow database work");
                }
            }))
            .map_err(|_| DbError::Closed);
        async move {
            sent?;
            result.await.map_err(|_| DbError::Closed)?
        }
    }
}

fn migrate(conn: &mut Connection, sets: &[Migrations]) -> Result<(), DbError> {
    let tx = conn.transaction()?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            component TEXT PRIMARY KEY,
            version INTEGER NOT NULL
        ) STRICT;",
    )?;
    for set in sets {
        let known = set.steps.len() as u32;
        let found: u32 = tx
            .query_row(
                "SELECT version FROM schema_migrations WHERE component = ?1",
                [set.component],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if found > known {
            return Err(DbError::TooNew {
                component: set.component,
                found,
                known,
            });
        }
        if found == known {
            continue;
        }
        for step in &set.steps[found as usize..] {
            tx.execute_batch(step)?;
        }
        tx.execute(
            "INSERT INTO schema_migrations (component, version) VALUES (?1, ?2)
             ON CONFLICT (component) DO UPDATE SET version = excluded.version",
            (set.component, known),
        )?;
        tracing::info!(
            component = set.component,
            from = found,
            to = known,
            "migrated"
        );
    }
    tx.commit()?;
    Ok(())
}

/// Reads a server-wide setting.
pub fn setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .optional()
}

/// Stores a server-wide setting, or forgets it when `value` is `None`.
pub fn set_setting(conn: &Connection, key: &str, value: Option<&str>) -> rusqlite::Result<()> {
    match value {
        Some(value) => conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            (key, value),
        )?,
        None => conn.execute("DELETE FROM settings WHERE key = ?1", [key])?,
    };
    Ok(())
}

/// Returns a setting, storing the value `init` makes the first time it is asked for.
pub fn setting_or_init(
    conn: &Connection,
    key: &str,
    init: impl FnOnce() -> String,
) -> rusqlite::Result<String> {
    if let Some(value) = setting(conn, key)? {
        return Ok(value);
    }
    let value = init();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)",
        (key, &value),
    )?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_sql_is_logged_on_one_short_line() {
        assert_eq!(
            one_line("SELECT a\n     FROM b\n  WHERE c = ?1"),
            "SELECT a FROM b WHERE c = ?1"
        );
        let long = one_line(&"x ".repeat(400));
        assert_eq!(long.chars().count(), 301);
        assert!(long.ends_with('…'));
    }

    const ITEMS: Migrations = Migrations {
        component: "items",
        steps: &["CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL) STRICT;"],
    };
    const ITEMS_V2: Migrations = Migrations {
        component: "items",
        steps: &[ITEMS.steps[0], "ALTER TABLE items ADD COLUMN year INTEGER;"],
    };

    fn version(conn: &Connection, component: &str) -> u32 {
        conn.query_row(
            "SELECT version FROM schema_migrations WHERE component = ?1",
            [component],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn migrates_a_fresh_database_and_records_versions() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &[CORE, ITEMS]).unwrap();
        assert_eq!(version(&conn, "core"), 1);
        assert_eq!(version(&conn, "items"), 1);
        conn.execute("INSERT INTO items (name) VALUES ('a')", [])
            .unwrap();
    }

    #[test]
    fn rerunning_applies_only_new_steps() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &[ITEMS]).unwrap();
        conn.execute("INSERT INTO items (name) VALUES ('kept')", [])
            .unwrap();
        migrate(&mut conn, &[ITEMS]).unwrap();
        migrate(&mut conn, &[ITEMS_V2]).unwrap();
        assert_eq!(version(&conn, "items"), 2);
        let (name, year): (String, Option<i64>) = conn
            .query_row("SELECT name, year FROM items", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!((name.as_str(), year), ("kept", None));
    }

    #[test]
    fn refuses_a_database_from_a_newer_server() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn, &[ITEMS_V2]).unwrap();
        assert!(matches!(
            migrate(&mut conn, &[ITEMS]),
            Err(DbError::TooNew {
                component: "items",
                found: 2,
                known: 1
            })
        ));
    }

    #[test]
    fn a_failing_step_leaves_no_trace() {
        const BROKEN: Migrations = Migrations {
            component: "broken",
            steps: &["CREATE TABLE fine (id INTEGER) STRICT;", "NOT SQL"],
        };
        let mut conn = Connection::open_in_memory().unwrap();
        assert!(migrate(&mut conn, &[ITEMS, BROKEN]).is_err());
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN ('items', 'fine')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0);
    }

    #[tokio::test]
    async fn settings_are_initialised_once_and_survive_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");

        let db = Database::open(&path, &[]).unwrap();
        let first = db
            .call(|conn| Ok(setting_or_init(conn, "server_id", || "one".to_owned())?))
            .await
            .unwrap();
        let second = db
            .call(|conn| Ok(setting_or_init(conn, "server_id", || "two".to_owned())?))
            .await
            .unwrap();
        assert_eq!((first.as_str(), second.as_str()), ("one", "one"));
        drop(db);

        let db = Database::open(&path, &[]).unwrap();
        let reopened = db
            .call(|conn| Ok(setting(conn, "server_id")?))
            .await
            .unwrap();
        assert_eq!(reopened.as_deref(), Some("one"));
    }
}
