//! Listening progress, kept on the server as its own apps keep it: through the session a book
//! opened when it started playing, synced every 20 seconds while it plays and at once when it
//! pauses or moves, and closed when it is left or ends, which marks it finished there.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bopsie_library::catalog::Progress;
use serde_json::json;
use tokio::sync::mpsc;

use crate::account::{Account, AccountError};
use crate::media::start_session;

/// How often a playing book's progress is synced.
pub const SYNC_EVERY: Duration = Duration::from_secs(20);

/// The session each book playing opened, by book: its id and the book's length in seconds.
#[derive(Clone, Default)]
pub struct Sessions(Arc<Mutex<HashMap<String, (String, f64)>>>);

impl Sessions {
    pub fn opened(&self, book: &str, session: &str, seconds: f64) {
        self.0
            .lock()
            .unwrap()
            .insert(book.to_owned(), (session.to_owned(), seconds));
    }

    fn get(&self, book: &str) -> Option<(String, f64)> {
        self.0.lock().unwrap().get(book).cloned()
    }

    fn forget(&self, book: &str) {
        self.0.lock().unwrap().remove(book);
    }
}

/// Listening to one book since it was last synced.
#[derive(Default)]
struct Listening {
    /// Since when it has played, while it plays.
    playing_since: Option<Instant>,
    /// Seconds listened since the last sync, before `playing_since`.
    listened: f64,
    last_sync: Option<Instant>,
}

/// Takes reports in order and keeps the server up to date with them.
pub fn start(account: Account, sessions: Sessions) -> mpsc::UnboundedSender<(String, Progress)> {
    let (reports, mut inbox) = mpsc::unbounded_channel::<(String, Progress)>();
    tokio::spawn(async move {
        let mut books: HashMap<String, Listening> = HashMap::new();
        while let Some((book, progress)) = inbox.recv().await {
            let listening = books.entry(book.clone()).or_default();
            let now = Instant::now();
            // Time listened is the time it played, whatever its speed, as the server counts it.
            if let Some(since) = listening.playing_since.take() {
                listening.listened += now.duration_since(since).as_secs_f64();
            }
            let result = match progress {
                Progress::Playing(position) => {
                    listening.playing_since = Some(now);
                    let due = listening
                        .last_sync
                        .is_none_or(|last| now.duration_since(last) >= SYNC_EVERY);
                    if due {
                        sync(&account, &sessions, &book, position, listening).await
                    } else {
                        Ok(())
                    }
                }
                Progress::Seeked(position) | Progress::Paused(position) => {
                    if matches!(progress, Progress::Seeked(_)) && listening.last_sync.is_some() {
                        // A seek while playing goes on playing.
                        listening.playing_since = Some(now);
                    }
                    sync(&account, &sessions, &book, position, listening).await
                }
                Progress::Stopped(position) => {
                    let result = close(&account, &sessions, &book, Some(position), listening).await;
                    books.remove(&book);
                    result
                }
                Progress::Ended => {
                    let result = close(&account, &sessions, &book, None, listening).await;
                    books.remove(&book);
                    result
                }
            };
            if let Err(error) = result {
                tracing::warn!(book, %error, "cannot keep listening progress on Audiobookshelf");
            }
        }
    });
    reports
}

/// The session `book` plays in: the one it opened, else a new one, as when the server forgot
/// it.
async fn session(
    account: &Account,
    sessions: &Sessions,
    book: &str,
) -> Result<(String, f64), AccountError> {
    if let Some(open) = sessions.get(book) {
        return Ok(open);
    }
    let opened = start_session(account, book)
        .await
        .map_err(|e| AccountError::Refused(e.to_string()))?;
    let seconds = opened.files.iter().map(|f| f.seconds).sum();
    sessions.opened(book, &opened.id, seconds);
    Ok((opened.id, seconds))
}

async fn sync(
    account: &Account,
    sessions: &Sessions,
    book: &str,
    position: Duration,
    listening: &mut Listening,
) -> Result<(), AccountError> {
    for attempt in 0..2 {
        let (id, seconds) = session(account, sessions, book).await?;
        let body = json!({
            "currentTime": position.as_secs_f64(),
            "timeListened": listening.listened,
            "duration": seconds,
        });
        match account.post(&format!("/api/session/{id}/sync"), body).await {
            Ok(_) => {
                listening.listened = 0.0;
                listening.last_sync = Some(Instant::now());
                return Ok(());
            }
            // The server forgot the session, as after it restarted: a new one, and again.
            Err(AccountError::NotFound(_)) if attempt == 0 => sessions.forget(book),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Closes `book`'s session at `position`, or at its end; one that never opened reports the
/// position as the book's progress.
async fn close(
    account: &Account,
    sessions: &Sessions,
    book: &str,
    position: Option<Duration>,
    listening: &mut Listening,
) -> Result<(), AccountError> {
    let open = sessions.get(book);
    sessions.forget(book);
    if let Some((id, seconds)) = open {
        let at = position.map_or(seconds, |p| p.as_secs_f64());
        let body = json!({
            "currentTime": at,
            "timeListened": listening.listened,
            "duration": seconds,
        });
        match account
            .post(&format!("/api/session/{id}/close"), body)
            .await
        {
            Ok(_) => return Ok(()),
            // Gone already: the progress is kept as progress.
            Err(AccountError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }
    let body = match position {
        Some(position) => json!({"currentTime": position.as_secs_f64()}),
        None => json!({"isFinished": true}),
    };
    account
        .patch(&format!("/api/me/progress/{book}"), body)
        .await
        .map(drop)
}
