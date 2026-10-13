//! When the next chunk may go to a client.
//!
//! Two limits apply. Buffer accounting: the bytes (header and payload) of every chunk still to
//! play at the client never exceed its `buffer_capacity`; a chunk counts until its end, less
//! the client's output delay, has passed. Fill rate: after an initial burst, audio goes out no
//! faster than four times real time, as aiosendspin sends it, so a fresh stream does not flood
//! the network or the client.

use std::collections::VecDeque;

use bopsie_core::clock::Micros;

/// How much faster than real time a stream may fill.
const FILL_RATE: i64 = 4;

#[derive(Debug, Clone)]
pub struct Pacer {
    capacity: u64,
    output_delay: Micros,
    /// Audio that may go out at once when a stream starts.
    burst: Micros,
    /// End time and size of each chunk still to play.
    outstanding: VecDeque<(Micros, u64)>,
    used: u64,
    /// The clock and the first chunk's timestamp when the stream (re)started.
    origin: Option<(Micros, Micros)>,
}

impl Pacer {
    pub fn new(capacity: u64, output_delay: Micros, burst: Micros) -> Self {
        Self {
            capacity,
            output_delay,
            burst,
            outstanding: VecDeque::new(),
            used: 0,
            origin: None,
        }
    }

    /// Forgets everything sent, as after `stream/clear` or `stream/end`.
    pub fn reset(&mut self) {
        self.outstanding.clear();
        self.used = 0;
        self.origin = None;
    }

    pub fn set_output_delay(&mut self, output_delay: Micros) {
        self.output_delay = output_delay;
    }

    fn prune(&mut self, now: Micros) {
        while let Some(&(end, bytes)) = self.outstanding.front() {
            if end > now {
                break;
            }
            self.outstanding.pop_front();
            self.used -= bytes;
        }
    }

    /// `None` when a chunk of `bytes` starting at `timestamp` and lasting `duration` may go
    /// now, or the time to ask again.
    pub fn ready_at(
        &mut self,
        now: Micros,
        timestamp: Micros,
        duration: Micros,
        bytes: u64,
    ) -> Option<Micros> {
        self.prune(now);
        let mut ready = now;

        // Room in the client's buffer, once enough of what it holds has played. A chunk larger
        // than the whole buffer goes alone.
        if self.used + bytes > self.capacity && !self.outstanding.is_empty() {
            let mut freed = 0;
            for &(end, size) in &self.outstanding {
                freed += size;
                if self.used - freed + bytes <= self.capacity {
                    ready = ready.max(end);
                    break;
                }
            }
        }

        let (start_clock, start_timestamp) = *self.origin.get_or_insert((now, timestamp));
        let ahead = timestamp + duration - start_timestamp - self.burst;
        if ahead > 0 {
            ready = ready.max(start_clock + ahead / FILL_RATE);
        }

        (ready > now).then_some(ready)
    }

    /// Counts a chunk as sent.
    pub fn sent(&mut self, timestamp: Micros, duration: Micros, bytes: u64) {
        self.outstanding
            .push_back((timestamp + duration - self.output_delay, bytes));
        self.used += bytes;
    }

    /// Bytes still to play at the client.
    pub fn used(&self) -> u64 {
        self.used
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK: Micros = 25_000;
    const BYTES: u64 = 4_809;

    /// Sends 25 ms chunks from `start` as fast as the pacer allows, with the clock jumping to
    /// each ready time. Returns when each chunk went.
    fn send(pacer: &mut Pacer, start: Micros, count: usize, mut now: Micros) -> Vec<Micros> {
        let mut sent = Vec::new();
        for i in 0..count as i64 {
            let timestamp = start + i * CHUNK;
            if let Some(ready) = pacer.ready_at(now, timestamp, CHUNK, BYTES) {
                now = ready;
                assert_eq!(pacer.ready_at(now, timestamp, CHUNK, BYTES), None);
            }
            pacer.sent(timestamp, CHUNK, BYTES);
            sent.push(now);
        }
        sent
    }

    #[test]
    fn the_burst_goes_at_once_then_four_times_real_time() {
        let mut pacer = Pacer::new(u64::MAX, 0, 300_000);
        let sent = send(&mut pacer, 1_300_000, 40, 1_000_000);
        // 12 chunks (300 ms) go immediately.
        assert!(sent[..12].iter().all(|&t| t == 1_000_000));
        // Then each 25 ms of audio waits 6.25 ms.
        assert_eq!(sent[12], 1_000_000 + CHUNK / 4);
        assert_eq!(sent[39], 1_000_000 + 28 * CHUNK / 4);
    }

    #[test]
    fn never_more_than_the_buffer_holds() {
        // Room for ten chunks; no burst limit to speak of.
        let mut pacer = Pacer::new(10 * BYTES, 0, Micros::MAX / 4);
        let start = 2_000_000;
        let sent = send(&mut pacer, start, 30, 1_000_000);
        assert!(sent[..10].iter().all(|&t| t == 1_000_000));
        // The eleventh waits for the first chunk to finish playing.
        assert_eq!(sent[10], start + CHUNK);
        assert_eq!(sent[29], start + 20 * CHUNK);
        assert!(pacer.used() <= 10 * BYTES);
    }

    #[test]
    fn output_delay_frees_room_earlier() {
        let mut pacer = Pacer::new(BYTES, 10_000, Micros::MAX / 4);
        let sent = send(&mut pacer, 2_000_000, 2, 1_000_000);
        assert_eq!(sent[1], 2_000_000 + CHUNK - 10_000);
    }

    #[test]
    fn a_reset_starts_a_fresh_burst() {
        let mut pacer = Pacer::new(u64::MAX, 0, 100_000);
        send(&mut pacer, 1_100_000, 20, 1_000_000);
        pacer.reset();
        assert_eq!(pacer.used(), 0);
        let sent = send(&mut pacer, 5_100_000, 4, 5_000_000);
        assert!(sent.iter().all(|&t| t == 5_000_000));
    }

    #[test]
    fn an_oversized_chunk_still_goes_into_an_empty_buffer() {
        let mut pacer = Pacer::new(100, 0, Micros::MAX / 4);
        assert_eq!(pacer.ready_at(0, 1_000, CHUNK, 1_000), None);
    }
}
