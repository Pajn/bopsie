//! `player_queues/*` commands. A queue's id is its player's id.

use std::path::PathBuf;

use bopsie_core::media::Track;
use bopsie_core::{Error, Result};
use bopsie_queues::{QueueCommand, QueueOption, RepeatMode};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::auth::Scope;
use crate::command::{Access, Command, Context};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueueArgs {
    pub queue_id: String,
}

pub struct Get;
impl Command for Get {
    const NAME: &'static str = "player_queues/get";
    const ACCESS: Access = Access::Scope(Scope::QueuesRead);
    const SUMMARY: &'static str = "A player's queue, or null when there is none.";
    type Args = QueueArgs;
    type Output = Option<Value>;

    async fn handle(ctx: Context, args: QueueArgs) -> Result<Option<Value>> {
        Ok(ctx.api.playback.queue(&args.queue_id))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ActiveQueueArgs {
    pub player_id: String,
}

pub struct GetActiveQueue;
impl Command for GetActiveQueue {
    const NAME: &'static str = "player_queues/get_active_queue";
    const ACCESS: Access = Access::Scope(Scope::QueuesRead);
    const SUMMARY: &'static str = "The queue a player plays from: its group leader's.";
    type Args = ActiveQueueArgs;
    type Output = Option<Value>;

    async fn handle(ctx: Context, args: ActiveQueueArgs) -> Result<Option<Value>> {
        let playback = &ctx.api.playback;
        Ok(playback
            .active_queue(&args.player_id)
            .and_then(|id| playback.queue(&id)))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ItemsArgs {
    pub queue_id: String,
    /// Defaults to 500.
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

pub struct Items;
impl Command for Items {
    const NAME: &'static str = "player_queues/items";
    const ACCESS: Access = Access::Scope(Scope::QueuesRead);
    const SUMMARY: &'static str = "A page of a queue's items, in play order.";
    type Args = ItemsArgs;
    type Output = Vec<Value>;

    async fn handle(ctx: Context, args: ItemsArgs) -> Result<Vec<Value>> {
        Ok(ctx
            .api
            .playback
            .queue_items(
                &args.queue_id,
                args.limit.unwrap_or(500),
                args.offset.unwrap_or(0),
            )
            .unwrap_or_default())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlayMediaArgs {
    pub queue_id: String,
    /// A URI such as `library://album/1`, a media item or item mapping with a `uri`, or a
    /// list of them.
    pub media: Value,
    /// `play`, `replace`, `next`, `replace_next` or `add`. Defaults to `play`.
    #[serde(default)]
    pub option: Option<String>,
    /// A track within the media to start from, by URI or item id.
    #[serde(default)]
    pub start_item: Option<Value>,
}

fn option(name: Option<&str>) -> Result<QueueOption> {
    Ok(match name.unwrap_or("play") {
        "play" => QueueOption::Play,
        "replace" => QueueOption::Replace,
        "next" => QueueOption::Next,
        "replace_next" => QueueOption::ReplaceNext,
        "add" => QueueOption::Add,
        other => return Err(Error::invalid_data(format!("Unknown option {other}"))),
    })
}

/// The URIs in `media`, whichever of its forms it takes.
fn uris(media: &Value) -> Vec<String> {
    match media {
        Value::String(uri) => vec![uri.clone()],
        Value::Array(items) => items.iter().flat_map(uris).collect(),
        Value::Object(item) => item
            .get("uri")
            .and_then(Value::as_str)
            .map(|uri| vec![uri.to_owned()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn matches(track: &Track, start: &Value) -> bool {
    let wanted: Vec<String> = match start {
        Value::Object(item) => ["uri", "item_id"]
            .iter()
            .filter_map(|key| item.get(*key)?.as_str().map(str::to_owned))
            .collect(),
        Value::String(text) => vec![text.clone()],
        _ => Vec::new(),
    };
    wanted
        .iter()
        .any(|w| *w == track.fields.uri || *w == track.fields.item_id)
}

pub struct PlayMedia;
impl Command for PlayMedia {
    const NAME: &'static str = "player_queues/play_media";
    const ACCESS: Access = Access::Scope(Scope::QueuesControl);
    const SUMMARY: &'static str =
        "Put media in a queue, as the option says, and play it if it says so.";
    type Args = PlayMediaArgs;
    type Output = ();

    async fn handle(ctx: Context, args: PlayMediaArgs) -> Result<()> {
        let option = option(args.option.as_deref())?;
        let mut tracks: Vec<(Track, PathBuf)> = Vec::new();
        for uri in uris(&args.media) {
            tracks.extend(ctx.api.library.playable(&uri).await?);
        }
        if tracks.is_empty() {
            return Err(Error::new(
                bopsie_core::ErrorCode::MediaNotFound,
                "Nothing to play",
            ));
        }
        let start_at = args
            .start_item
            .as_ref()
            .and_then(|start| tracks.iter().position(|(track, _)| matches(track, start)));
        ctx.api
            .playback
            .play_media(&args.queue_id, tracks, option, start_at)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlayIndexArgs {
    pub queue_id: String,
    /// An index into the queue, or a queue item id.
    pub index: Value,
}

pub struct PlayIndex;
impl Command for PlayIndex {
    const NAME: &'static str = "player_queues/play_index";
    const ACCESS: Access = Access::Scope(Scope::QueuesControl);
    const SUMMARY: &'static str = "Play the queue from an item.";
    type Args = PlayIndexArgs;
    type Output = ();

    async fn handle(ctx: Context, args: PlayIndexArgs) -> Result<()> {
        let item = match &args.index {
            Value::String(id) => id.clone(),
            Value::Number(index) => index.to_string(),
            _ => return Err(Error::invalid_data("index must be a number or an item id")),
        };
        ctx.api.playback.play_index(&args.queue_id, &item)
    }
}

macro_rules! simple_commands {
    ($($name:ident, $command:literal, $variant:expr, $summary:literal;)*) => {$(
        pub struct $name;
        impl Command for $name {
            const NAME: &'static str = $command;
            const ACCESS: Access = Access::Scope(Scope::QueuesControl);
            const SUMMARY: &'static str = $summary;
            type Args = QueueArgs;
            type Output = ();

            async fn handle(ctx: Context, args: QueueArgs) -> Result<()> {
                ctx.api.playback.command(&args.queue_id, $variant)
            }
        }
    )*};
}

simple_commands! {
    Play, "player_queues/play", QueueCommand::Play, "Play, or resume where the queue paused.";
    Pause, "player_queues/pause", QueueCommand::Pause, "Pause.";
    PlayPause, "player_queues/play_pause", QueueCommand::PlayPause, "Pause when playing, play otherwise.";
    Stop, "player_queues/stop", QueueCommand::Stop, "Stop.";
    Next, "player_queues/next", QueueCommand::Next, "Play the next item.";
    Previous, "player_queues/previous", QueueCommand::Previous,
        "Play the previous item, or the current one from its start after five seconds.";
    Clear, "player_queues/clear", QueueCommand::Clear, "Empty the queue and stop.";
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SeekArgs {
    pub queue_id: String,
    /// Seconds into the current item.
    pub position: f64,
}

pub struct Seek;
impl Command for Seek {
    const NAME: &'static str = "player_queues/seek";
    const ACCESS: Access = Access::Scope(Scope::QueuesControl);
    const SUMMARY: &'static str = "Move to a position in the current item.";
    type Args = SeekArgs;
    type Output = ();

    async fn handle(ctx: Context, args: SeekArgs) -> Result<()> {
        ctx.api
            .playback
            .command(&args.queue_id, QueueCommand::Seek(args.position))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SkipArgs {
    pub queue_id: String,
    /// Seconds forward, or back when negative. Defaults to 10.
    #[serde(default)]
    pub seconds: Option<f64>,
}

pub struct Skip;
impl Command for Skip {
    const NAME: &'static str = "player_queues/skip";
    const ACCESS: Access = Access::Scope(Scope::QueuesControl);
    const SUMMARY: &'static str = "Move forward or back within the current item.";
    type Args = SkipArgs;
    type Output = ();

    async fn handle(ctx: Context, args: SkipArgs) -> Result<()> {
        ctx.api.playback.command(
            &args.queue_id,
            QueueCommand::Skip(args.seconds.unwrap_or(10.0)),
        )
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ShuffleArgs {
    pub queue_id: String,
    pub shuffle_enabled: bool,
}

pub struct Shuffle;
impl Command for Shuffle {
    const NAME: &'static str = "player_queues/shuffle";
    const ACCESS: Access = Access::Scope(Scope::QueuesControl);
    const SUMMARY: &'static str = "Shuffle what is still to play, or restore its order.";
    type Args = ShuffleArgs;
    type Output = ();

    async fn handle(ctx: Context, args: ShuffleArgs) -> Result<()> {
        ctx.api
            .playback
            .command(&args.queue_id, QueueCommand::Shuffle(args.shuffle_enabled))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RepeatArgs {
    pub queue_id: String,
    /// `off`, `one` or `all`.
    pub repeat_mode: String,
}

pub struct Repeat;
impl Command for Repeat {
    const NAME: &'static str = "player_queues/repeat";
    const ACCESS: Access = Access::Scope(Scope::QueuesControl);
    const SUMMARY: &'static str = "Repeat nothing, the current item, or the whole queue.";
    type Args = RepeatArgs;
    type Output = ();

    async fn handle(ctx: Context, args: RepeatArgs) -> Result<()> {
        let mode = match args.repeat_mode.as_str() {
            "off" => RepeatMode::Off,
            "one" => RepeatMode::One,
            "all" => RepeatMode::All,
            other => return Err(Error::invalid_data(format!("Unknown repeat mode {other}"))),
        };
        ctx.api
            .playback
            .command(&args.queue_id, QueueCommand::Repeat(mode))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MoveItemArgs {
    pub queue_id: String,
    pub queue_item_id: String,
    /// Places down when positive, up when negative; 0 moves it to play next. Defaults to 1.
    #[serde(default)]
    pub pos_shift: Option<i64>,
}

pub struct MoveItem;
impl Command for MoveItem {
    const NAME: &'static str = "player_queues/move_item";
    const ACCESS: Access = Access::Scope(Scope::QueuesControl);
    const SUMMARY: &'static str =
        "Move an item. Items the player already holds cannot be moved, or moved past.";
    type Args = MoveItemArgs;
    type Output = ();

    async fn handle(ctx: Context, args: MoveItemArgs) -> Result<()> {
        ctx.api.playback.move_item(
            &args.queue_id,
            &args.queue_item_id,
            args.pos_shift.unwrap_or(1),
        )
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteItemArgs {
    pub queue_id: String,
    /// A queue item id, or an index into the queue.
    pub item_id_or_index: Value,
}

pub struct DeleteItem;
impl Command for DeleteItem {
    const NAME: &'static str = "player_queues/delete_item";
    const ACCESS: Access = Access::Scope(Scope::QueuesControl);
    const SUMMARY: &'static str = "Remove an item. Items the player already holds stay.";
    type Args = DeleteItemArgs;
    type Output = ();

    async fn handle(ctx: Context, args: DeleteItemArgs) -> Result<()> {
        let item = match &args.item_id_or_index {
            Value::String(id) => id.clone(),
            Value::Number(index) => index.to_string(),
            _ => {
                return Err(Error::invalid_data(
                    "item_id_or_index must be an id or an index",
                ));
            }
        };
        ctx.api.playback.delete_item(&args.queue_id, &item)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TransferArgs {
    pub source_queue_id: String,
    pub target_queue_id: String,
}

pub struct Transfer;
impl Command for Transfer {
    const NAME: &'static str = "player_queues/transfer";
    const ACCESS: Access = Access::Scope(Scope::QueuesControl);
    const SUMMARY: &'static str = "Move a queue, its position and settings to another player, which plays on if the source was playing.";
    type Args = TransferArgs;
    type Output = ();

    async fn handle(ctx: Context, args: TransferArgs) -> Result<()> {
        ctx.api
            .playback
            .transfer(&args.source_queue_id, &args.target_queue_id)
    }
}
