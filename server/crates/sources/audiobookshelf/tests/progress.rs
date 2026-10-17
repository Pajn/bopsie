//! Listening progress kept on a stand-in server, as playback reports it.

mod common;

use std::time::{Duration, Instant};

use bopsie_core::db::Database;
use bopsie_core::events::EventBus;
use bopsie_library::Library;
use bopsie_library::catalog::{Catalog, Progress};
use bopsie_sources_audiobookshelf::{Account, AudiobookshelfCatalog};
use serde_json::Value;

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// Waits until the stand-in was told `count` things, and answers with them.
async fn reports(stand: &common::Shared, count: usize) -> Vec<(String, Value)> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let reports = stand.lock().unwrap().reports.clone();
        if reports.len() >= count {
            return reports;
        }
        assert!(Instant::now() < deadline, "only {reports:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn progress_is_synced_through_the_books_session_and_closed_where_it_was_left() {
    let (url, stand) = common::start().await;
    // A book of two files, 100 s in all.
    stand.lock().unwrap().files = vec![("a".into(), 60.0), ("b".into(), 40.0)];
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_in_memory(&[bopsie_library::MIGRATIONS]).unwrap();
    let library = Library::new(
        db.clone(),
        EventBus::default(),
        Vec::new(),
        dir.path().into(),
    );
    let account = Account::open(db).await.unwrap();
    account
        .sign_in(&url, "reader", common::PASSWORD)
        .await
        .unwrap();
    let catalog = AudiobookshelfCatalog::new(account, library, None);

    // Starting: a session, synced at once.
    catalog.progress("li_h", Progress::Playing(secs(10)));
    let first = reports(&stand, 1).await;
    assert_eq!(first[0].0, "sync ses1");
    assert_eq!(first[0].1["currentTime"], 10.0);
    assert_eq!(first[0].1["duration"], 100.0);
    // Playing on: not again until 20 s have passed.
    catalog.progress("li_h", Progress::Playing(secs(11)));
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    // Pausing and seeking sync at once; the time listened counts the time it played.
    catalog.progress("li_h", Progress::Paused(secs(12)));
    catalog.progress("li_h", Progress::Seeked(secs(50)));
    let all = reports(&stand, 3).await;
    assert_eq!(all.len(), 3, "{all:?}");
    assert_eq!(all[1].1["currentTime"], 12.0);
    let listened = all[1].1["timeListened"].as_f64().unwrap();
    assert!((1.0..3.0).contains(&listened), "{listened}");
    assert_eq!(all[2].1["currentTime"], 50.0);
    assert_eq!(all[2].1["timeListened"], 0.0, "paused meanwhile");

    // The server restarted and forgot the session: a new one.
    stand.lock().unwrap().forgotten.push("ses1".to_owned());
    catalog.progress("li_h", Progress::Seeked(secs(60)));
    let all = reports(&stand, 4).await;
    assert_eq!(all[3].0, "sync ses2");
    assert_eq!(all[3].1["currentTime"], 60.0);

    // Left part-way: the session closed there.
    catalog.progress("li_h", Progress::Stopped(secs(70)));
    let all = reports(&stand, 5).await;
    assert_eq!(all[4].0, "close ses2");
    assert_eq!(all[4].1["currentTime"], 70.0);

    // A book that ends with no session open is marked finished as its progress.
    catalog.progress("li_a", Progress::Ended);
    let all = reports(&stand, 6).await;
    assert_eq!(all[5].0, "progress li_a");
    assert_eq!(all[5].1["isFinished"], true);
}
