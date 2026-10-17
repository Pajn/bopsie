//! Signing in to Audiobookshelf and browsing its books, against a stand-in server.

mod common;

use bopsie_core::db::Database;
use bopsie_core::events::EventBus;
use bopsie_core::media::{MediaItem, MediaType};
use bopsie_library::Library;
use bopsie_library::catalog::Catalog;
use bopsie_sources_audiobookshelf::{Account, AccountError, AudiobookshelfCatalog};

use common::PASSWORD;

struct Fixture {
    db: Database,
    account: Account,
    catalog: AudiobookshelfCatalog,
    library: Library,
    url: String,
    stand: common::Shared,
    _dir: tempfile::TempDir,
}

async fn fixture() -> Fixture {
    let (url, stand) = common::start().await;
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_in_memory(&[bopsie_library::MIGRATIONS]).unwrap();
    let library = Library::new(
        db.clone(),
        EventBus::default(),
        Vec::new(),
        dir.path().join("artwork"),
    );
    let account = Account::open(db.clone()).await.unwrap();
    let catalog = AudiobookshelfCatalog::new(account.clone(), library.clone(), None);
    Fixture {
        db,
        account,
        catalog,
        library,
        url,
        stand,
        _dir: dir,
    }
}

impl Fixture {
    async fn signed_in(self) -> Self {
        self.account
            .sign_in(&format!("{}/", self.url), "reader", PASSWORD)
            .await
            .unwrap();
        self
    }

    /// Every setting the database holds, as `key=value` lines.
    async fn settings(&self) -> String {
        self.db
            .call(|conn| {
                let mut statement = conn.prepare("SELECT key, value FROM settings")?;
                let lines: Vec<String> = statement
                    .query_map([], |row| {
                        Ok(format!(
                            "{}={}",
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?
                        ))
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                Ok(lines.join("\n"))
            })
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn signing_in_keeps_the_tokens_and_never_the_password() {
    let f = fixture().await;
    assert!(!f.catalog.available());
    let wrong = f.account.sign_in(&f.url, "reader", "nope").await;
    assert!(matches!(wrong, Err(AccountError::Refused(_))), "{wrong:?}");
    assert!(!f.account.signed_in());

    let f = f.signed_in().await;
    let status = f.account.status();
    assert!(status.signed_in);
    assert_eq!(status.user.as_deref(), Some("reader"));
    // The trailing slash is dropped.
    assert_eq!(status.url.as_deref(), Some(f.url.as_str()));
    assert!(f.catalog.available());
    let settings = f.settings().await;
    assert!(
        settings.contains("audiobookshelf.access_token=access-1"),
        "{settings}"
    );
    assert!(
        settings.contains("audiobookshelf.refresh_token=refresh-1"),
        "{settings}"
    );
    assert!(
        !settings.contains(PASSWORD),
        "the password is kept: {settings}"
    );

    // Opened again, it is still signed in.
    let again = Account::open(f.db.clone()).await.unwrap();
    assert!(again.signed_in());

    f.account.sign_out().await.unwrap();
    assert!(!f.account.signed_in());
    assert_eq!(f.account.status().user.as_deref(), Some("reader"));
    assert!(!f.settings().await.contains("access-1"));
}

#[tokio::test]
async fn an_expired_token_is_refreshed_once_and_a_refused_refresh_signs_out() {
    let f = fixture().await.signed_in().await;
    // The server lets the access token go.
    f.stand.lock().unwrap().access = "access-2".to_owned();
    f.stand.lock().unwrap().refreshes = 0;
    assert_eq!(f.catalog.audiobook_count().await.unwrap(), 3);
    assert_eq!(f.stand.lock().unwrap().refreshes, 1);
    let settings = f.settings().await;
    assert!(
        settings.contains("audiobookshelf.access_token=access-2"),
        "{settings}"
    );
    assert!(
        settings.contains("audiobookshelf.refresh_token=refresh-2"),
        "{settings}"
    );

    {
        let mut stand = f.stand.lock().unwrap();
        stand.access = "gone".to_owned();
        stand.refuse_refresh = true;
    }
    assert!(f.catalog.audiobook_count().await.is_err());
    assert!(!f.account.signed_in());
    assert!(!f.catalog.available());
}

#[tokio::test]
async fn books_page_with_their_authors_narrators_and_covers() {
    let f = fixture().await.signed_in().await;
    assert_eq!(f.catalog.audiobook_count().await.unwrap(), 3);
    let all = f.catalog.audiobooks(0, 10).await.unwrap();
    let names: Vec<&str> = all.iter().map(|b| b.fields.name.as_str()).collect();
    assert_eq!(names, ["Anathem", "The Hobbit", "Snow Crash"]);
    let hobbit = &all[1];
    assert_eq!(hobbit.fields.sort_name, "hobbit");
    assert_eq!(hobbit.fields.uri, "audiobookshelf://audiobook/li_h");
    assert_eq!(hobbit.authors[0].name, "J. R. R. Tolkien");
    assert_eq!(hobbit.authors[0].uri, "audiobookshelf://artist/aut2");
    assert_eq!(hobbit.narrators, ["Nora Narrator"]);
    assert_eq!(hobbit.publisher.as_deref(), Some("Allen & Unwin"));
    assert_eq!(hobbit.duration, 7_201);
    let chapters = hobbit.fields.metadata.chapters.as_ref().unwrap();
    assert_eq!(chapters[1].name, "Two");
    assert_eq!(chapters[1].start_ms, 3_600_250);
    assert_eq!(
        hobbit.fields.metadata.description.as_deref(),
        Some("A book.")
    );

    // Paging in even steps asks the server for that page.
    let second = f.catalog.audiobooks(2, 2).await.unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].fields.name, "Snow Crash");
    assert!(
        f.stand
            .lock()
            .unwrap()
            .requests
            .contains(&"GET /api/libraries/lib1/items?limit=2&page=1".to_owned())
    );
    assert_eq!(
        f.catalog.audiobooks(1, 2).await.unwrap()[0].fields.name,
        "The Hobbit"
    );
    assert!(f.catalog.audiobooks(3, 2).await.unwrap().is_empty());

    // Its cover through the image proxy, fetched from the server.
    let cover = &hobbit.fields.metadata.images.as_ref().unwrap()[0];
    let bytes = f.library.image(cover.proxy_id.clone()).await.unwrap();
    assert_eq!(bytes, None, "not yet a catalog of the library's");
    f.library
        .catalogs()
        .add(std::sync::Arc::new(AudiobookshelfCatalog::new(
            f.account.clone(),
            f.library.clone(),
            None,
        )));
    let bytes = f.library.image(cover.proxy_id.clone()).await.unwrap();
    assert_eq!(bytes.as_deref(), Some(&b"cover of li_h"[..]));
}

#[tokio::test]
async fn a_book_on_its_own_says_where_listening_left_off() {
    let f = fixture().await.signed_in().await;
    let MediaItem::Audiobook(book) = f.catalog.item(MediaType::Audiobook, "li_h").await.unwrap()
    else {
        panic!("not a book");
    };
    assert_eq!(book.resume_position_ms, Some(600_250));
    assert_eq!(book.fully_played, Some(false));
    let MediaItem::Audiobook(fresh) = f.catalog.item(MediaType::Audiobook, "li_a").await.unwrap()
    else {
        panic!("not a book");
    };
    assert_eq!(fresh.resume_position_ms, None);
    assert!(f.catalog.item(MediaType::Audiobook, "li_x").await.is_err());
}

#[tokio::test]
async fn books_and_authors_are_found_and_an_authors_books_listed() {
    let f = fixture().await.signed_in().await;
    let books = f
        .catalog
        .search("crash", MediaType::Audiobook, 10)
        .await
        .unwrap();
    let names: Vec<&str> = books.iter().map(|b| b.fields().name.as_str()).collect();
    assert_eq!(names, ["Snow Crash"]);
    let authors = f
        .catalog
        .search("neal", MediaType::Artist, 10)
        .await
        .unwrap();
    assert_eq!(authors[0].fields().uri, "audiobookshelf://artist/aut1");
    assert!(
        f.catalog
            .search("neal", MediaType::Track, 10)
            .await
            .unwrap()
            .is_empty()
    );
    let MediaItem::Artist(neal) = f.catalog.item(MediaType::Artist, "aut1").await.unwrap() else {
        panic!("not an artist");
    };
    assert_eq!(
        neal.fields.metadata.description.as_deref(),
        Some("Writes books.")
    );
    let theirs = f.catalog.artist_audiobooks("aut1").await.unwrap();
    let names: Vec<&str> = theirs.iter().map(|b| b.fields.name.as_str()).collect();
    assert_eq!(names, ["Anathem", "Snow Crash"]);
}

#[tokio::test]
async fn a_library_of_ebooks_holds_no_audiobooks() {
    let f = fixture().await.signed_in().await;
    // Three books; the ebooks' library is left out, and the server asked once which is which.
    assert_eq!(f.catalog.audiobook_count().await.unwrap(), 3);
    let names: Vec<String> = f
        .catalog
        .audiobooks(0, 10)
        .await
        .unwrap()
        .into_iter()
        .map(|b| b.fields.name)
        .collect();
    assert_eq!(names, ["Anathem", "The Hobbit", "Snow Crash"]);
    let asked = |requests: &[String]| {
        requests
            .iter()
            .filter(|r| r.contains("filter=tracks"))
            .count()
    };
    assert_eq!(
        asked(&f.stand.lock().unwrap().requests),
        2,
        "once per library"
    );
    // An author's ebooks are not among their audiobooks.
    let theirs: Vec<String> = f
        .catalog
        .artist_audiobooks("aut1")
        .await
        .unwrap()
        .into_iter()
        .map(|b| b.fields.name)
        .collect();
    assert_eq!(theirs, ["Anathem", "Snow Crash"]);
}

#[tokio::test]
async fn listening_continues_in_the_book_listened_to_last() {
    let f = fixture().await.signed_in().await;
    let started = f.catalog.in_progress(10).await.unwrap();
    let names: Vec<&str> = started.iter().map(|b| b.fields.name.as_str()).collect();
    // Not the finished book, nor the ebook being read.
    assert_eq!(names, ["Snow Crash", "The Hobbit"]);
    assert_eq!(started[0].resume_position_ms, Some(90_500));
    assert_eq!(started[1].resume_position_ms, Some(600_250));
    assert_eq!(started[1].fully_played, Some(false));
    assert_eq!(f.catalog.in_progress(1).await.unwrap().len(), 1);
    assert!(f.catalog.in_progress(0).await.unwrap().is_empty());
}
