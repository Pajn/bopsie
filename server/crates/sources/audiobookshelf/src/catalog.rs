//! Audiobookshelf's books as a catalog: browsed and searched as the server has them, its
//! authors as artists, and its covers through the library's image proxy.

use std::path::PathBuf;
use std::sync::Arc;

use bopsie_audio::engine::Input;
use bopsie_core::media::{
    Album, Artist, Audiobook, Chapter, Image, ItemFields, ItemMapping, MediaItem, MediaType,
    Metadata, Track, sort_name,
};
use bopsie_library::Library;
use bopsie_library::catalog::{self, Catalog, CatalogError, CatalogResult};
use bopsie_library::scan::proxy_id;
use futures::future::BoxFuture;
use serde_json::Value;
use tokio::runtime::Handle;

use crate::account::{Account, AccountError};
use crate::media::BookMedia;

/// The catalog's provider id, and its URIs' scheme.
pub const AUDIOBOOKSHELF: &str = "audiobookshelf";

pub struct AudiobookshelfCatalog {
    account: Account,
    library: Library,
    /// What plays books: they are fetched from the server through it.
    ffmpeg: Option<PathBuf>,
    runtime: Handle,
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
        Self {
            account,
            library,
            ffmpeg,
            runtime: Handle::current(),
        }
    }

    /// The server's libraries of books, by id.
    async fn book_libraries(&self) -> CatalogResult<Vec<String>> {
        let answer = self.account.get("/api/libraries").await?;
        Ok(answer["libraries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|library| library["mediaType"] == "book")
            .filter_map(|library| library["id"].as_str().map(str::to_owned))
            .collect())
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
            .filter(|item| item["mediaType"] == "book")
            .filter_map(audiobook)
            .collect();
        self.remember(books.iter().map(|book| &book.fields)).await;
        books
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
            for library in self.book_libraries().await? {
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
                        self.books(&items)
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
                    self.remember(authors.iter().map(|a| &a.fields)).await;
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
                    self.remember([&artist.fields]).await;
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
        ))))
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
            for library in self.book_libraries().await? {
                if left == 0 {
                    break;
                }
                let total = self.page(&library, 1, 0).await?["total"]
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
                let answer = self.page(&library, size, page).await?;
                let items: Vec<Value> = answer["results"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .skip(drop)
                    .take(left)
                    .collect();
                skip = 0;
                let page = self.books(&items).await;
                left = left.saturating_sub(page.len());
                books.extend(page);
            }
            Ok(books)
        })
    }

    fn audiobook_count<'a>(&'a self) -> BoxFuture<'a, CatalogResult<usize>> {
        Box::pin(async move {
            let mut count = 0;
            for library in self.book_libraries().await? {
                count += self.page(&library, 1, 0).await?["total"]
                    .as_u64()
                    .unwrap_or(0) as usize;
            }
            Ok(count)
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
            Ok(self.books(&items).await)
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
