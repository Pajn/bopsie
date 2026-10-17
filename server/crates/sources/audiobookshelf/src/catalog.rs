//! Audiobookshelf's books as a catalog: browsed and searched as the server has them, its
//! authors as artists, and its covers through the library's image proxy.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bopsie_audio::engine::Input;
use bopsie_core::media::{
    Album, Artist, Audiobook, Chapter, Image, ItemFields, ItemMapping, MediaItem, MediaType,
    Metadata, Track, sort_name,
};
use bopsie_library::Library;
use bopsie_library::catalog::{self, Catalog, CatalogError, CatalogResult, Progress};
use bopsie_library::scan::proxy_id;
use futures::future::BoxFuture;
use serde_json::Value;
use tokio::runtime::Handle;
use tokio::sync::mpsc;

use crate::account::{Account, AccountError};
use crate::media::BookMedia;
use crate::progress::{self, Sessions};

/// The catalog's provider id, and its URIs' scheme.
pub const AUDIOBOOKSHELF: &str = "audiobookshelf";

/// How long which libraries hold audiobooks is kept before the server is asked again.
const LIBRARIES_KEPT: Duration = Duration::from_secs(600);

/// How many books listening continues in are kept; a shelf asks for fewer.
const CONTINUING_KEPT: usize = 24;
/// How long the books listening continues in are answered as kept before they are asked again,
/// in the background.
const CONTINUING_FRESH: Duration = Duration::from_secs(60);
/// How long after listening moves on they are asked again: time for the server to hear of it.
const CONTINUING_AFTER_PROGRESS: Duration = Duration::from_secs(5);
/// How long after a book starts they are asked again: after the first sync of where it is.
const CONTINUING_AFTER_START: Duration = Duration::from_secs(25);

/// The server's filter for items with no audio files: `tracks.` and `none` in base64, encoded
/// for a URL.
const NO_AUDIO: &str = "tracks.bm9uZQ%3D%3D";

pub struct AudiobookshelfCatalog {
    account: Account,
    /// What plays books: they are fetched from the server through it.
    ffmpeg: Option<PathBuf>,
    runtime: Handle,
    sessions: Sessions,
    /// Where listening progress goes to be kept on the server.
    progress: mpsc::UnboundedSender<(String, Progress)>,
    source: Arc<Source>,
}

/// What reads the server's books, shared with the catalog's work in the background.
struct Source {
    account: Account,
    library: Library,
    /// The libraries that hold audiobooks, the server they are of, and when they were asked.
    libraries: Mutex<Option<(String, Instant, Vec<String>)>>,
    /// The books listening continues in, as last asked.
    continuing: Mutex<Option<Continuing>>,
    /// A refresh of `continuing` runs.
    refreshing: AtomicBool,
    /// A refresh of `continuing` is wanted.
    wanted: AtomicBool,
    /// The book playback last said plays.
    playing: Mutex<Option<String>>,
    runtime: Handle,
}

/// The books listening continues in, kept to answer at once.
struct Continuing {
    /// The server and the user they are of.
    of: (Option<String>, Option<String>),
    at: Instant,
    books: Vec<Audiobook>,
    /// They are out of date: listening moved on since.
    stale: bool,
}

impl From<AccountError> for CatalogError {
    fn from(error: AccountError) -> Self {
        match error {
            AccountError::NotFound(what) => CatalogError::NotFound(what),
            other => CatalogError::Unavailable(other.to_string()),
        }
    }
}

fn fields(media_type: MediaType, id: &str, name: &str, sort: Option<&str>) -> ItemFields {
    ItemFields {
        item_id: id.to_owned(),
        provider: AUDIOBOOKSHELF.to_owned(),
        name: name.to_owned(),
        version: String::new(),
        sort_name: sort.map_or_else(|| sort_name(name), str::to_lowercase),
        uri: catalog::uri(AUDIOBOOKSHELF, media_type, id),
        external_ids: Vec::new(),
        is_playable: true,
        media_type,
        provider_mappings: Vec::new(),
        metadata: Metadata::default(),
        favorite: false,
        position: None,
        date_added: None,
    }
}

fn mapping(media_type: MediaType, id: &str, name: &str) -> ItemMapping {
    ItemMapping {
        item_id: id.to_owned(),
        provider: AUDIOBOOKSHELF.to_owned(),
        name: name.to_owned(),
        version: String::new(),
        sort_name: sort_name(name),
        uri: catalog::uri(AUDIOBOOKSHELF, media_type, id),
        external_ids: Vec::new(),
        is_playable: true,
        media_type,
        available: true,
        image: None,
        year: None,
    }
}

/// An image the server serves at `path`, through the image proxy.
fn image(path: String) -> Image {
    Image {
        kind: "thumb",
        proxy_id: proxy_id(AUDIOBOOKSHELF, &path),
        path,
        provider: AUDIOBOOKSHELF.to_owned(),
        remotely_accessible: false,
    }
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// A library item of the server's as an audiobook: its metadata, its chapters and, with
/// progress, where listening left off.
pub fn audiobook(item: &Value) -> Option<Audiobook> {
    let id = item["id"].as_str()?;
    let media = &item["media"];
    let metadata = &media["metadata"];
    let title = text(&metadata["title"]).unwrap_or_else(|| id.to_owned());
    let mut fields = fields(
        MediaType::Audiobook,
        id,
        &title,
        metadata["titleIgnorePrefix"].as_str(),
    );
    fields.version = text(&metadata["subtitle"]).unwrap_or_default();
    fields.metadata.description =
        text(&metadata["descriptionPlain"]).or_else(|| text(&metadata["description"]));
    fields.metadata.genres = metadata["genres"]
        .as_array()
        .map(|genres| genres.iter().filter_map(text).collect())
        .filter(|genres: &Vec<String>| !genres.is_empty());
    fields.metadata.release_date = text(&metadata["publishedYear"]);
    fields.metadata.languages = text(&metadata["language"]).map(|language| vec![language]);
    if media["coverPath"].as_str().is_some() {
        fields.metadata.images = Some(vec![image(format!("items/{id}/cover"))]);
    }
    let chapters: Vec<Chapter> = media["chapters"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(position, chapter)| Chapter {
            position: position as u32,
            name: text(&chapter["title"]).unwrap_or_else(|| format!("Chapter {}", position + 1)),
            start_ms: seconds_to_ms(&chapter["start"]),
            end_ms: chapter["end"]
                .as_f64()
                .map(|_| seconds_to_ms(&chapter["end"])),
        })
        .collect();
    if !chapters.is_empty() {
        fields.metadata.chapters = Some(chapters);
    }
    // A list names its authors in one line, without ids; a book on its own lists them.
    let authors = match metadata["authors"].as_array() {
        Some(authors) => authors
            .iter()
            .filter_map(|author| {
                Some(mapping(
                    MediaType::Artist,
                    author["id"].as_str()?,
                    author["name"].as_str()?,
                ))
            })
            .collect(),
        None => Vec::new(),
    };
    let narrators = match metadata["narrators"].as_array() {
        Some(narrators) => narrators.iter().filter_map(text).collect(),
        None => text(&metadata["narratorName"])
            .map(|names| names.split(", ").map(str::to_owned).collect())
            .unwrap_or_default(),
    };
    let progress = item
        .get("userMediaProgress")
        .or_else(|| item.get("mediaProgress"))
        .filter(|p| p.is_object());
    Some(Audiobook {
        fields,
        publisher: text(&metadata["publisher"]),
        authors,
        narrators,
        duration: media["duration"].as_f64().unwrap_or(0.0).round() as u32,
        fully_played: progress.map(|p| p["isFinished"].as_bool().unwrap_or(false)),
        resume_position_ms: progress
            .map(|p| seconds_to_ms(&p["currentTime"]))
            .filter(|ms| *ms > 0),
    })
}

/// Whether a library item has audio to play: an ebook has none.
fn has_audio(item: &Value) -> bool {
    let media = &item["media"];
    media["numAudioFiles"].as_u64() != Some(0) && media["duration"].as_f64() != Some(0.0)
}

fn seconds_to_ms(seconds: &Value) -> u64 {
    (seconds.as_f64().unwrap_or(0.0).max(0.0) * 1_000.0).round() as u64
}

/// An author of the server's as an artist.
pub fn author(value: &Value) -> Option<Artist> {
    let id = value["id"].as_str()?;
    let name = value["name"].as_str()?;
    let mut fields = fields(MediaType::Artist, id, name, None);
    fields.metadata.description = text(&value["description"]);
    if value["imagePath"].as_str().is_some() {
        fields.metadata.images = Some(vec![image(format!("authors/{id}/image"))]);
    }
    Some(Artist {
        fields,
        artist_type: "artist",
    })
}

impl AudiobookshelfCatalog {
    /// The catalog of `account`'s server; books play through `ffmpeg`, and without it do not.
    /// Needs a Tokio runtime.
    pub fn new(account: Account, library: Library, ffmpeg: Option<PathBuf>) -> Self {
        let sessions = Sessions::default();
        let source = Arc::new(Source {
            account: account.clone(),
            library: library.clone(),
            libraries: Mutex::default(),
            continuing: Mutex::default(),
            refreshing: AtomicBool::default(),
            wanted: AtomicBool::default(),
            playing: Mutex::default(),
            runtime: Handle::current(),
        });
        // Kept from the start, for the first to ask to be answered at once.
        if account.signed_in() {
            source.refresh_soon(Duration::ZERO);
        }
        Self {
            progress: progress::start(account.clone(), sessions.clone()),
            account,
            ffmpeg,
            runtime: Handle::current(),
            sessions,
            source,
        }
    }
}

impl Source {
    /// The server's libraries of audiobooks, by id: its libraries of books most of whose items
    /// have audio. A library of ebooks is a library of books too, with none.
    async fn book_libraries(&self) -> CatalogResult<Vec<String>> {
        let server = self.account.status().url.unwrap_or_default();
        if let Some((of, at, libraries)) = &*self.libraries.lock().unwrap()
            && *of == server
            && at.elapsed() < LIBRARIES_KEPT
        {
            return Ok(libraries.clone());
        }
        let answer = self.account.get("/api/libraries").await?;
        let mut libraries = Vec::new();
        for library in answer["libraries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|library| library["mediaType"] == "book")
            .filter_map(|library| library["id"].as_str())
        {
            let total = |answer: Value| answer["total"].as_u64().unwrap_or(0);
            let all = total(self.page(library, 1, 0).await?);
            let silent = total(
                self.account
                    .get(&format!(
                        "/api/libraries/{library}/items?limit=1&filter={NO_AUDIO}"
                    ))
                    .await?,
            );
            if silent * 2 < all {
                libraries.push(library.to_owned());
            }
        }
        *self.libraries.lock().unwrap() = Some((server, Instant::now(), libraries.clone()));
        Ok(libraries)
    }

    /// Lets the image proxy serve the images `items` name.
    async fn remember<'a>(&self, items: impl IntoIterator<Item = &'a ItemFields>) {
        let images: Vec<(String, String)> = items
            .into_iter()
            .flat_map(|fields| fields.metadata.images.iter().flatten())
            .map(|image| (image.provider.clone(), image.path.clone()))
            .collect();
        if let Err(error) = self.library.remember_images(images).await {
            tracing::warn!(%error, "cannot keep Audiobookshelf's images");
        }
    }

    async fn books(&self, items: &[Value]) -> Vec<Audiobook> {
        let books: Vec<Audiobook> = items
            .iter()
            .filter(|item| item["mediaType"] == "book" && has_audio(item))
            .filter_map(audiobook)
            .collect();
        self.remember(books.iter().map(|book| &book.fields)).await;
        books
    }

    /// The books listening continues in, the most recently listened to first: at most
    /// `limit`, from each library's continue listening.
    async fn fetch_in_progress(&self, limit: usize) -> CatalogResult<Vec<Audiobook>> {
        // Where listening is in each book, and when it was last listened to.
        let me = self.account.get("/api/me").await?;
        let progress: HashMap<&str, &Value> = me["mediaProgress"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|progress| progress["episodeId"].is_null())
            .filter_map(|progress| Some((progress["libraryItemId"].as_str()?, progress)))
            .collect();
        // Each library's continue listening, as the server's own apps show it: without
        // what was finished or hidden from it.
        let mut started: Vec<(u64, Value)> = Vec::new();
        for library in self.book_libraries().await? {
            let shelves = self
                .account
                .get(&format!(
                    "/api/libraries/{library}/personalized?limit={limit}"
                ))
                .await?;
            let shelf = shelves
                .as_array()
                .into_iter()
                .flatten()
                .find(|shelf| shelf["id"] == "continue-listening");
            for item in shelf
                .and_then(|shelf| shelf["entities"].as_array())
                .into_iter()
                .flatten()
            {
                let Some(&listened) = item["id"].as_str().and_then(|id| progress.get(id)) else {
                    continue;
                };
                let mut item = item.clone();
                item["mediaProgress"] = listened.clone();
                started.push((listened["lastUpdate"].as_u64().unwrap_or(0), item));
            }
        }
        started.sort_by_key(|(last, _)| std::cmp::Reverse(*last));
        let items: Vec<Value> = started.into_iter().map(|(_, item)| item).collect();
        let mut books = self.books(&items).await;
        books.truncate(limit);
        Ok(books)
    }

    /// Whom the books listening continues in are of: the server and the user signed in.
    fn listener(&self) -> (Option<String>, Option<String>) {
        let status = self.account.status();
        (status.url, status.user)
    }

    /// The books listening continues in as kept for whoever is signed in now, and whether they
    /// are to be asked again.
    fn kept_continuing(&self) -> Option<(Vec<Audiobook>, bool)> {
        let listener = self.listener();
        let continuing = self.continuing.lock().unwrap();
        let kept = continuing.as_ref().filter(|kept| kept.of == listener)?;
        Some((
            kept.books.clone(),
            kept.stale || kept.at.elapsed() >= CONTINUING_FRESH,
        ))
    }

    /// Asks the server for the books listening continues in and keeps them. When they changed
    /// from those kept, the clients are told, to show them again.
    async fn refresh_continuing(&self) -> CatalogResult<Vec<Audiobook>> {
        let of = self.listener();
        let books = self.fetch_in_progress(CONTINUING_KEPT).await?;
        let before = self.continuing.lock().unwrap().replace(Continuing {
            of: of.clone(),
            at: Instant::now(),
            books: books.clone(),
            stale: false,
        });
        let changed = before.is_some_and(|before| before.of == of && before.books != books);
        if changed && let Some(first) = books.first() {
            match serde_json::to_value(first) {
                Ok(item) => self
                    .library
                    .announce_updated(first.fields.uri.clone(), item),
                Err(error) => tracing::warn!(%error, "cannot announce an audiobook"),
            }
        }
        Ok(books)
    }

    /// Asks again for the books listening continues in, `after` from now, in the background:
    /// once, however often it is wanted meanwhile, and once more if it is wanted while asking.
    fn refresh_soon(self: &Arc<Self>, after: Duration) {
        self.wanted.store(true, Ordering::SeqCst);
        if self.refreshing.swap(true, Ordering::SeqCst) {
            return;
        }
        let source = self.clone();
        self.runtime.spawn(async move {
            tokio::time::sleep(after).await;
            while source.wanted.swap(false, Ordering::SeqCst) {
                if let Err(error) = source.refresh_continuing().await {
                    tracing::debug!(%error, "cannot ask Audiobookshelf what listening continues in");
                }
            }
            source.refreshing.store(false, Ordering::SeqCst);
            // Wanted between the last ask and letting go: asked for again.
            if source.wanted.load(Ordering::SeqCst) {
                source.refresh_soon(Duration::ZERO);
            }
        });
    }

    /// Playback told how far into book `id` listening is: the books listening continues in
    /// are asked again once the server has heard, when it changes what they are.
    fn listening_moved(self: &Arc<Self>, id: &str, progress: &Progress) {
        let after = match progress {
            // Every second while it plays: only a book that was not playing moves it first,
            // once the server has its first sync.
            Progress::Playing(_) => {
                let mut playing = self.playing.lock().unwrap();
                if playing.as_deref() == Some(id) {
                    return;
                }
                *playing = Some(id.to_owned());
                CONTINUING_AFTER_START
            }
            Progress::Paused(_) | Progress::Seeked(_) | Progress::Ended | Progress::Stopped(_) => {
                if !matches!(progress, Progress::Seeked(_)) {
                    *self.playing.lock().unwrap() = None;
                }
                CONTINUING_AFTER_PROGRESS
            }
        };
        if let Some(kept) = self.continuing.lock().unwrap().as_mut() {
            kept.stale = true;
        }
        self.refresh_soon(after);
    }

    /// A page of one library's books, by title.
    async fn page(&self, library: &str, limit: usize, page: usize) -> CatalogResult<Value> {
        Ok(self
            .account
            .get(&format!(
                "/api/libraries/{library}/items?limit={limit}&page={page}&sort=media.metadata.title&include=progress"
            ))
            .await?)
    }
}

impl Catalog for AudiobookshelfCatalog {
    fn instance_id(&self) -> &str {
        AUDIOBOOKSHELF
    }

    fn domain(&self) -> &str {
        AUDIOBOOKSHELF
    }

    fn name(&self) -> &str {
        "Audiobookshelf"
    }

    fn available(&self) -> bool {
        self.account.signed_in()
    }

    fn search<'a>(
        &'a self,
        query: &'a str,
        media_type: MediaType,
        limit: usize,
    ) -> BoxFuture<'a, CatalogResult<Vec<MediaItem>>> {
        Box::pin(async move {
            if !matches!(media_type, MediaType::Audiobook | MediaType::Artist) {
                return Ok(Vec::new());
            }
            let mut found = Vec::new();
            for library in self.source.book_libraries().await? {
                let answer = self
                    .account
                    .get(&format!(
                        "/api/libraries/{library}/search?q={}&limit={limit}",
                        urlencode(query)
                    ))
                    .await?;
                if media_type == MediaType::Audiobook {
                    let items: Vec<Value> = answer["book"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|hit| hit["libraryItem"].clone())
                        .collect();
                    found.extend(
                        self.source
                            .books(&items)
                            .await
                            .into_iter()
                            .map(|book| MediaItem::Audiobook(Box::new(book))),
                    );
                } else {
                    let authors: Vec<Artist> = answer["authors"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(author)
                        .collect();
                    self.source
                        .remember(authors.iter().map(|a| &a.fields))
                        .await;
                    found.extend(authors.into_iter().map(|a| MediaItem::Artist(Box::new(a))));
                }
            }
            found.truncate(limit);
            Ok(found)
        })
    }

    fn item<'a>(
        &'a self,
        media_type: MediaType,
        id: &'a str,
    ) -> BoxFuture<'a, CatalogResult<MediaItem>> {
        Box::pin(async move {
            match media_type {
                MediaType::Audiobook => {
                    let item = self
                        .account
                        .get(&format!("/api/items/{id}?expanded=1&include=progress"))
                        .await?;
                    let book = self
                        .source
                        .books(std::slice::from_ref(&item))
                        .await
                        .pop()
                        .ok_or_else(|| CatalogError::NotFound(format!("audiobook {id}")))?;
                    Ok(MediaItem::Audiobook(Box::new(book)))
                }
                MediaType::Artist => {
                    let value = self.account.get(&format!("/api/authors/{id}")).await?;
                    let artist = author(&value)
                        .ok_or_else(|| CatalogError::NotFound(format!("author {id}")))?;
                    self.source.remember([&artist.fields]).await;
                    Ok(MediaItem::Artist(Box::new(artist)))
                }
                _ => Err(CatalogError::NotFound(format!(
                    "Audiobookshelf {} {id}",
                    media_type.as_str()
                ))),
            }
        })
    }

    fn album_tracks<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Track>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn artist_albums<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Album>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn artist_tracks<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Track>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn playlist_tracks<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, CatalogResult<Vec<Track>>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn media(&self, id: &str) -> Option<Input> {
        let Some(ffmpeg) = &self.ffmpeg else {
            tracing::warn!(
                book = id,
                "Audiobookshelf's books play through ffmpeg, which is not installed"
            );
            return None;
        };
        Some(Input::Open(Arc::new(BookMedia::new(
            self.account.clone(),
            id.to_owned(),
            ffmpeg.clone(),
            self.runtime.clone(),
            self.sessions.clone(),
        ))))
    }

    fn progress(&self, id: &str, progress: Progress) {
        self.source.listening_moved(id, &progress);
        let _ = self.progress.send((id.to_owned(), progress));
    }

    fn audiobooks<'a>(
        &'a self,
        offset: usize,
        limit: usize,
    ) -> BoxFuture<'a, CatalogResult<Vec<Audiobook>>> {
        Box::pin(async move {
            let mut books = Vec::new();
            let mut skip = offset;
            let mut left = limit;
            for library in self.source.book_libraries().await? {
                if left == 0 {
                    break;
                }
                let total = self.source.page(&library, 1, 0).await?["total"]
                    .as_u64()
                    .unwrap_or(0) as usize;
                if skip >= total {
                    skip -= total;
                    continue;
                }
                // The server pages by page number: a page of `left` when `skip` falls on one,
                // as it does for a client paging in steps of the same size; else from the
                // start, enough to cover what is wanted.
                let (size, page, drop) = if skip.is_multiple_of(left) {
                    (left, skip / left, 0)
                } else {
                    (skip + left, 0, skip)
                };
                let answer = self.source.page(&library, size, page).await?;
                let items: Vec<Value> = answer["results"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .skip(drop)
                    .take(left)
                    .collect();
                skip = 0;
                let page = self.source.books(&items).await;
                left = left.saturating_sub(page.len());
                books.extend(page);
            }
            Ok(books)
        })
    }

    fn audiobook_count<'a>(&'a self) -> BoxFuture<'a, CatalogResult<usize>> {
        Box::pin(async move {
            let mut count = 0;
            for library in self.source.book_libraries().await? {
                count += self.source.page(&library, 1, 0).await?["total"]
                    .as_u64()
                    .unwrap_or(0) as usize;
            }
            Ok(count)
        })
    }

    fn in_progress<'a>(&'a self, limit: usize) -> BoxFuture<'a, CatalogResult<Vec<Audiobook>>> {
        Box::pin(async move {
            if limit == 0 {
                return Ok(Vec::new());
            }
            if limit > CONTINUING_KEPT {
                return self.source.fetch_in_progress(limit).await;
            }
            // Kept, it answers at once, and is asked again in the background when it is old.
            let books = match self.source.kept_continuing() {
                Some((books, old)) => {
                    if old {
                        self.source.refresh_soon(Duration::ZERO);
                    }
                    books
                }
                None => self.source.refresh_continuing().await?,
            };
            Ok(books.into_iter().take(limit).collect())
        })
    }

    fn artist_audiobooks<'a>(
        &'a self,
        id: &'a str,
    ) -> BoxFuture<'a, CatalogResult<Vec<Audiobook>>> {
        Box::pin(async move {
            let answer = self
                .account
                .get(&format!("/api/authors/{id}?include=items"))
                .await?;
            let items = answer["libraryItems"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            Ok(self.source.books(&items).await)
        })
    }

    fn image<'a>(&'a self, path: &'a str) -> BoxFuture<'a, CatalogResult<Option<Vec<u8>>>> {
        Box::pin(async move {
            match self.account.bytes(&format!("/api/{path}")).await {
                Ok(bytes) => Ok(Some(bytes)),
                Err(AccountError::NotFound(_)) => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
    }
}

/// `query` as a URL query value.
fn urlencode(query: &str) -> String {
    query
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
