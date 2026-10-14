//! The log's latest lines, kept in memory for the web UI to tail. A `tracing` layer: it sees
//! what the log's filter lets through, as the printed log does.

use std::collections::{BTreeMap, VecDeque};
use std::fmt::{self, Write as _};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// Lines kept, the oldest dropped first.
pub const KEPT: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Line {
    /// Counts up from 1, so a reader asks for what came after the last it has.
    pub seq: u64,
    pub unix_ms: u64,
    /// `ERROR`, `WARN`, `INFO`, `DEBUG` or `TRACE`.
    pub level: String,
    /// The module that logged it.
    pub target: String,
    pub message: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
struct Lines {
    next: u64,
    lines: VecDeque<Line>,
}

/// The kept lines, shared by the layer that writes them and whoever reads them.
#[derive(Debug, Clone, Default)]
pub struct Tail {
    inner: Arc<Mutex<Lines>>,
}

impl Tail {
    /// Lines after `seq`, oldest first, at most `limit` of the newest.
    pub fn after(&self, seq: u64, limit: usize) -> Vec<Line> {
        let inner = self.inner.lock().unwrap();
        let newer: Vec<&Line> = inner.lines.iter().filter(|l| l.seq > seq).collect();
        newer[newer.len().saturating_sub(limit)..]
            .iter()
            .map(|line| (*line).clone())
            .collect()
    }

    fn push(&self, mut line: Line) {
        let mut inner = self.inner.lock().unwrap();
        inner.next += 1;
        line.seq = inner.next;
        if inner.lines.len() == KEPT {
            inner.lines.pop_front();
        }
        inner.lines.push_back(line);
    }
}

#[derive(Default)]
struct Fields {
    message: String,
    fields: BTreeMap<String, String>,
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let mut text = String::new();
        let _ = write!(text, "{value:?}");
        self.put(field, text);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, value.to_owned());
    }
}

impl Fields {
    fn put(&mut self, field: &Field, value: String) {
        if field.name() == "message" {
            self.message = value;
        } else {
            self.fields.insert(field.name().to_owned(), value);
        }
    }
}

impl<S: Subscriber> Layer<S> for Tail {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let metadata = event.metadata();
        self.push(Line {
            seq: 0,
            unix_ms: crate::clock::unix_ms(crate::clock::now()),
            level: metadata.level().to_string(),
            target: metadata.target().to_owned(),
            message: fields.message,
            fields: fields.fields,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn lines_are_kept_in_order_and_read_after_a_point() {
        let tail = Tail::default();
        let subscriber = tracing_subscriber::registry().with(tail.clone());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(speaker = "kitchen", volume = 30, "volume set");
            tracing::warn!("second");
            for i in 0..KEPT {
                tracing::debug!(i, "filler");
            }
        });

        let all = tail.after(0, usize::MAX);
        assert_eq!(all.len(), KEPT, "the oldest are dropped");
        assert_eq!(all[0].seq, 3);
        assert_eq!(all.last().unwrap().seq, KEPT as u64 + 2);
        assert!(all.windows(2).all(|w| w[0].seq + 1 == w[1].seq));

        let latest = tail.after(KEPT as u64, 10);
        assert_eq!(latest.len(), 2);
        assert_eq!(latest[0].seq, KEPT as u64 + 1);
        assert_eq!(latest[1].fields["i"], (KEPT - 1).to_string());
        assert_eq!(tail.after(0, 5).len(), 5, "the newest, when limited");
        assert!(tail.after(KEPT as u64 + 2, 10).is_empty());
    }

    #[test]
    fn a_line_has_its_message_level_and_fields() {
        let tail = Tail::default();
        let subscriber = tracing_subscriber::registry().with(tail.clone());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(speaker = "kitchen", volume = 30, "volume set");
        });
        let line = &tail.after(0, 10)[0];
        assert_eq!(line.seq, 1);
        assert_eq!(line.level, "INFO");
        assert_eq!(line.message, "volume set");
        assert_eq!(line.fields["speaker"], "kitchen");
        assert_eq!(line.fields["volume"], "30");
        assert!(line.target.ends_with("logtail::tests"), "{}", line.target);
    }
}
