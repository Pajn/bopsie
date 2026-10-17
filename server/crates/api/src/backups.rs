//! The server's backups: made on request or on a schedule into the backups directory, and
//! restored by staging one and restarting, so that it is moved into place before the database
//! opens.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bopsie_backup::{Backup, BackupError};
use bopsie_core::db::{self, Database, Migrations};
use bopsie_core::{Error, ErrorCode, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::sync::Notify;

/// The schedule, in the settings table.
const SCHEDULE: &str = "backup.schedule";
/// The passphrase scheduled backups are encrypted with, in the settings table.
const PASSPHRASE: &str = "backup.passphrase";
/// How often the schedule is looked at.
const CHECK_EVERY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Every {
    Daily,
    Weekly,
}

/// When backups are made by themselves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Schedule {
    pub every: Every,
    /// The hour of the day, in UTC.
    pub hour: u8,
    /// For a weekly one, the day: 0 for Monday to 6 for Sunday, in UTC.
    #[serde(default)]
    pub weekday: u8,
    /// How many scheduled backups are kept; older ones are removed.
    pub keep: u32,
    /// When it was set, in Unix seconds: no backup is due from before.
    pub since: i64,
}

impl Schedule {
    /// The time a backup was last due at, at or before `now`.
    pub fn last_due(&self, now: OffsetDateTime) -> OffsetDateTime {
        let hour = time::Time::from_hms(self.hour.min(23), 0, 0).expect("an hour of the day");
        let mut due = now.to_offset(time::UtcOffset::UTC).replace_time(hour);
        if due > now {
            due -= time::Duration::DAY;
        }
        if self.every == Every::Weekly {
            let behind = (due.weekday().number_days_from_monday() + 7 - self.weekday.min(6)) % 7;
            due -= time::Duration::days(i64::from(behind));
        }
        due
    }
}

/// How the last scheduled backup went.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Run {
    /// Unix seconds.
    pub at: i64,
    /// The backup made, or why none was.
    pub backup: Option<String>,
    pub error: Option<String>,
}

#[derive(Default)]
struct State {
    /// A backup is being made.
    making: bool,
    last_run: Option<Run>,
}

/// The server's backups.
#[derive(Clone)]
pub struct Backups {
    db: Database,
    data_dir: PathBuf,
    dir: PathBuf,
    /// The schema this server brings, which a backup restored must not be newer than.
    known: Arc<Vec<Migrations>>,
    version: String,
    state: Arc<Mutex<State>>,
    restart: Arc<Notify>,
}

/// Backup errors as the API reports them.
fn error(error: BackupError) -> Error {
    let code = match &error {
        BackupError::PassphraseNeeded | BackupError::WrongPassphrase => {
            ErrorCode::AuthenticationFailed
        }
        BackupError::NotABackup(_) | BackupError::TooNew { .. } => ErrorCode::InvalidData,
        BackupError::NoSuchBackup(_) => ErrorCode::MediaNotFound,
        BackupError::Io(_) | BackupError::Db(_) => {
            tracing::error!(%error, "backup failed");
            ErrorCode::Generic
        }
    };
    Error::new(code, error.to_string())
}

fn busy() -> Error {
    Error::new(ErrorCode::ResourceBusy, "A backup is being made")
}

impl Backups {
    pub fn new(
        db: Database,
        data_dir: PathBuf,
        dir: PathBuf,
        known: Vec<Migrations>,
        version: String,
    ) -> Self {
        Self {
            db,
            data_dir,
            dir,
            known: Arc::new(known),
            version,
            state: Arc::default(),
            restart: Arc::default(),
        }
    }

    /// Where backups are kept.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Resolves when a restore asks for the server to restart.
    pub async fn restart_requested(&self) {
        self.restart.notified().await;
    }

    pub fn list(&self) -> Result<Vec<Backup>> {
        bopsie_backup::list(&self.dir).map_err(error)
    }

    pub fn making(&self) -> bool {
        self.state.lock().unwrap().making
    }

    pub fn last_run(&self) -> Option<Run> {
        self.state.lock().unwrap().last_run.clone()
    }

    /// Makes a backup now, encrypted with `passphrase` when there is one.
    pub async fn create(&self, passphrase: Option<&str>, scheduled: bool) -> Result<Backup> {
        {
            let mut state = self.state.lock().unwrap();
            if state.making {
                return Err(busy());
            }
            state.making = true;
        }
        let made = bopsie_backup::create(
            &self.db,
            &self.data_dir,
            &self.dir,
            passphrase,
            scheduled,
            &self.version,
        )
        .await;
        self.state.lock().unwrap().making = false;
        let backup = made.map_err(error)?;
        tracing::info!(
            name = backup.name,
            bytes = backup.size,
            encrypted = backup.encrypted,
            "backup written"
        );
        Ok(backup)
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        bopsie_backup::delete(&self.dir, name).map_err(error)
    }

    /// Where backup `name` is, to be downloaded.
    pub fn path_of(&self, name: &str) -> Result<PathBuf> {
        bopsie_backup::path_of(&self.dir, name).map_err(error)
    }

    /// Where an uploaded backup is written before it is restored.
    pub fn upload_path(&self) -> PathBuf {
        self.data_dir.join(".restore-upload")
    }

    /// Restores the backup at `archive`: checks and stages it, then asks the server to restart,
    /// which moves it into place.
    pub async fn restore(&self, archive: PathBuf, passphrase: Option<String>) -> Result<()> {
        if self.making() {
            return Err(busy());
        }
        let (data_dir, known) = (self.data_dir.clone(), self.known.clone());
        let manifest = tokio::task::spawn_blocking(move || {
            bopsie_backup::stage(&archive, passphrase.as_deref(), &data_dir, &known)
        })
        .await
        .map_err(|_| Error::unexpected())?
        .map_err(error)?;
        tracing::info!(
            created = manifest.created,
            version = manifest.server_version,
            "restarting to restore a backup"
        );
        self.restart.notify_one();
        Ok(())
    }

    pub async fn schedule(&self) -> Result<Option<Schedule>> {
        let value = self
            .db
            .call(|conn| Ok(db::setting(conn, SCHEDULE)?))
            .await?;
        Ok(value.and_then(|value| serde_json::from_str(&value).ok()))
    }

    pub async fn has_passphrase(&self) -> Result<bool> {
        Ok(self.passphrase().await?.is_some())
    }

    async fn passphrase(&self) -> Result<Option<String>> {
        Ok(self
            .db
            .call(|conn| Ok(db::setting(conn, PASSPHRASE)?))
            .await?)
    }

    /// Sets the schedule, or turns it off; `passphrase` sets the scheduled backups' passphrase
    /// when given, and removes it when empty.
    pub async fn set_schedule(
        &self,
        schedule: Option<Schedule>,
        passphrase: Option<String>,
    ) -> Result<()> {
        let schedule = schedule
            .map(|schedule| serde_json::to_string(&schedule))
            .transpose()
            .map_err(|_| Error::unexpected())?;
        self.db
            .call(move |conn| {
                db::set_setting(conn, SCHEDULE, schedule.as_deref())?;
                if let Some(passphrase) = passphrase {
                    let passphrase = Some(passphrase).filter(|p| !p.is_empty());
                    db::set_setting(conn, PASSPHRASE, passphrase.as_deref())?;
                }
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Makes the scheduled backups, for as long as the server runs: when one is due and none
    /// was made since, as after the server was down at the time, then prunes them.
    pub async fn run_schedule(self) {
        loop {
            if let Err(error) = self.scheduled_once(OffsetDateTime::now_utc()).await {
                tracing::warn!(%error, "cannot run the backup schedule");
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    }

    /// Makes a scheduled backup if one is due at `now`.
    pub async fn scheduled_once(&self, now: OffsetDateTime) -> Result<Option<Backup>> {
        let Some(schedule) = self.schedule().await? else {
            return Ok(None);
        };
        let due = schedule.last_due(now).unix_timestamp();
        let newest = self
            .list()?
            .into_iter()
            .filter(|backup| backup.scheduled)
            .map(|backup| backup.created)
            .max();
        if due < schedule.since || newest.is_some_and(|newest| newest >= due) || self.making() {
            return Ok(None);
        }
        let passphrase = self.passphrase().await?;
        let made = self.create(passphrase.as_deref(), true).await;
        let run = Run {
            at: now.unix_timestamp(),
            backup: made.as_ref().ok().map(|backup| backup.name.clone()),
            error: made.as_ref().err().map(|error| error.details.clone()),
        };
        self.state.lock().unwrap().last_run = Some(run);
        match made {
            Ok(backup) => {
                let removed = bopsie_backup::prune(&self.dir, schedule.keep.max(1) as usize)
                    .map_err(error)?;
                for name in removed {
                    tracing::info!(name, "old backup removed");
                }
                Ok(Some(backup))
            }
            Err(error) => {
                tracing::error!(error = error.details, "backup failed");
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn schedule(every: Every, hour: u8, weekday: u8) -> Schedule {
        Schedule {
            every,
            hour,
            weekday,
            keep: 7,
            since: 0,
        }
    }

    #[test]
    fn a_daily_backup_was_due_at_its_hour_today_or_yesterday() {
        let daily = schedule(Every::Daily, 3, 0);
        assert_eq!(
            daily.last_due(datetime!(2026-10-17 18:00 UTC)),
            datetime!(2026-10-17 03:00 UTC)
        );
        assert_eq!(
            daily.last_due(datetime!(2026-10-17 02:59 UTC)),
            datetime!(2026-10-16 03:00 UTC)
        );
        assert_eq!(
            daily.last_due(datetime!(2026-10-17 03:00 UTC)),
            datetime!(2026-10-17 03:00 UTC)
        );
    }

    #[test]
    fn a_weekly_backup_was_due_on_its_day() {
        // 17 October 2026 is a Saturday; Monday is 0.
        let mondays = schedule(Every::Weekly, 3, 0);
        assert_eq!(
            mondays.last_due(datetime!(2026-10-17 18:00 UTC)),
            datetime!(2026-10-12 03:00 UTC)
        );
        let saturdays = schedule(Every::Weekly, 3, 5);
        assert_eq!(
            saturdays.last_due(datetime!(2026-10-17 18:00 UTC)),
            datetime!(2026-10-17 03:00 UTC)
        );
        assert_eq!(
            saturdays.last_due(datetime!(2026-10-17 02:00 UTC)),
            datetime!(2026-10-10 03:00 UTC)
        );
    }
}
