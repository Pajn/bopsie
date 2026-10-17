//! The server's backups, from the web UI: making one, the schedule, and restoring one kept in
//! the backups directory. Downloading one and restoring an uploaded one go over HTTP, as files.

use bopsie_core::{Error, ErrorCode, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::auth::Scope;
use crate::backups::{Every, Run, Schedule};
use crate::command::{Access, Command, Context};
use crate::commands::auth::NoArgs;

/// A backup kept in the backups directory.
#[derive(Debug, Serialize, JsonSchema)]
pub struct BackupView {
    pub name: String,
    /// Bytes.
    pub size: u64,
    /// Unix seconds.
    pub created: i64,
    pub encrypted: bool,
    /// Made on the schedule, which removes the oldest of them.
    pub scheduled: bool,
}

impl From<bopsie_backup::Backup> for BackupView {
    fn from(backup: bopsie_backup::Backup) -> Self {
        Self {
            name: backup.name,
            size: backup.size,
            created: backup.created,
            encrypted: backup.encrypted,
            scheduled: backup.scheduled,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BackupsStatus {
    /// Where backups are kept, on the server.
    pub dir: String,
    /// Newest first.
    pub backups: Vec<BackupView>,
    /// None when backups are not made by themselves.
    pub schedule: Option<Schedule>,
    /// Scheduled backups are encrypted with a passphrase kept on the server.
    pub encrypted: bool,
    pub last_run: Option<Run>,
    /// A backup is being made now.
    pub making: bool,
}

pub struct Status;
impl Command for Status {
    const NAME: &'static str = "admin/backups/status";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str =
        "The server's backups, their schedule, and how the last scheduled one went.";
    type Args = NoArgs;
    type Output = BackupsStatus;

    async fn handle(ctx: Context, _: NoArgs) -> Result<BackupsStatus> {
        let backups = &ctx.api.backups;
        Ok(BackupsStatus {
            dir: backups.dir().display().to_string(),
            backups: backups.list()?.into_iter().map(Into::into).collect(),
            schedule: backups.schedule().await?,
            encrypted: backups.has_passphrase().await?,
            last_run: backups.last_run(),
            making: backups.making(),
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateArgs {
    /// Encrypts it with this passphrase, which is needed to restore it and kept nowhere.
    #[serde(default)]
    pub passphrase: Option<String>,
}

pub struct Create;
impl Command for Create {
    const NAME: &'static str = "admin/backups/create";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Make a backup of the whole server now, into the backups directory; GET /backups/<name> downloads it.";
    type Args = CreateArgs;
    type Output = BackupView;

    async fn handle(ctx: Context, args: CreateArgs) -> Result<BackupView> {
        let passphrase = args.passphrase.filter(|p| !p.is_empty());
        Ok(ctx
            .api
            .backups
            .create(passphrase.as_deref(), false)
            .await?
            .into())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NameArgs {
    pub name: String,
}

pub struct Delete;
impl Command for Delete {
    const NAME: &'static str = "admin/backups/delete";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Delete a backup from the backups directory.";
    type Args = NameArgs;
    type Output = ();

    async fn handle(ctx: Context, args: NameArgs) -> Result<()> {
        ctx.api.backups.delete(&args.name)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScheduleArgs {
    /// How often; left out, backups are not made by themselves.
    #[serde(default)]
    pub every: Option<Every>,
    /// The hour of the day, in UTC.
    #[serde(default)]
    pub hour: u8,
    /// For a weekly one, the day, in UTC: 0 for Monday to 6 for Sunday.
    #[serde(default)]
    pub weekday: u8,
    /// How many scheduled backups are kept. Defaults to 7.
    #[serde(default)]
    pub keep: Option<u32>,
    /// Encrypts scheduled backups with this passphrase, kept on the server to do so; empty
    /// stops encrypting them. Left out, it stays as it is.
    #[serde(default)]
    pub passphrase: Option<String>,
}

pub struct SetSchedule;
impl Command for SetSchedule {
    const NAME: &'static str = "admin/backups/schedule";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str =
        "Make backups by themselves, daily or weekly at an hour, keeping the newest; or stop.";
    type Args = ScheduleArgs;
    type Output = ();

    async fn handle(ctx: Context, args: ScheduleArgs) -> Result<()> {
        if args.hour > 23 || args.weekday > 6 || args.keep == Some(0) {
            return Err(Error::new(
                ErrorCode::InvalidData,
                "hour is 0 to 23, weekday 0 to 6, and keep at least 1",
            ));
        }
        let schedule = args.every.map(|every| Schedule {
            every,
            hour: args.hour,
            weekday: args.weekday,
            keep: args.keep.unwrap_or(7),
            since: time::OffsetDateTime::now_utc().unix_timestamp(),
        });
        ctx.api
            .backups
            .set_schedule(schedule, args.passphrase)
            .await
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RestoreArgs {
    /// A backup in the backups directory.
    pub name: String,
    /// Its passphrase, when it is encrypted.
    #[serde(default)]
    pub passphrase: Option<String>,
}

pub struct Restore;
impl Command for Restore {
    const NAME: &'static str = "admin/backups/restore";
    const ACCESS: Access = Access::Scope(Scope::Admin);
    const SUMMARY: &'static str = "Restore a backup from the backups directory, replacing the whole server's state: the server checks it, then restarts, keeping what it replaced in before-restore-<time> in the data directory.";
    type Args = RestoreArgs;
    type Output = ();

    async fn handle(ctx: Context, args: RestoreArgs) -> Result<()> {
        let path = ctx.api.backups.path_of(&args.name)?;
        ctx.api
            .backups
            .restore(path, args.passphrase.filter(|p| !p.is_empty()))
            .await
    }
}
