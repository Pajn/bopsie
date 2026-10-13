package expo.modules.cratieplayer

import org.junit.Assert.assertEquals
import org.junit.Test

class SchedulerTest {
  private val rate = 48_000
  private val frames = 1_200 // 25 ms

  @Test fun `snaps at the start of a stream with silence before early audio`() {
    // Heard 200 ms after the cursor: 9600 frames of silence first.
    assertEquals(Alignment(silenceFrames = 9_600), Scheduler.align(1_200_000, 1_000_000, frames, rate, snap = true))
  }

  @Test fun `snaps by dropping the late part of a chunk`() {
    assertEquals(Alignment(dropFrames = 480), Scheduler.align(1_000_000, 1_010_000, frames, rate, snap = true))
  }

  @Test fun `drops a chunk that is late beyond its own length`() {
    assertEquals(Alignment(dropFrames = frames), Scheduler.align(1_000_000, 1_100_000, frames, rate, snap = false))
  }

  @Test fun `plays a chunk within tolerance as it is`() {
    assertEquals(Alignment(), Scheduler.align(1_000_050, 1_000_000, frames, rate, snap = false))
  }

  @Test fun `corrects small errors by at most half a percent of the chunk`() {
    // 1 ms early: 48 frames of error, corrected 6 at a time.
    assertEquals(Alignment(repeatFrames = 6), Scheduler.align(1_001_000, 1_000_000, frames, rate, snap = false))
    // 1 ms late.
    assertEquals(Alignment(dropFrames = 6), Scheduler.align(1_000_000, 1_001_000, frames, rate, snap = false))
    // Under the cap, the whole error at once: 120 µs is 5 frames.
    assertEquals(Alignment(repeatFrames = 5), Scheduler.align(1_000_120, 1_000_000, frames, rate, snap = false))
  }

  @Test fun `snaps mid-stream when the error is large, as after an underrun`() {
    assertEquals(Alignment(silenceFrames = 1_440), Scheduler.align(1_030_000, 1_000_000, frames, rate, snap = false))
  }
}
