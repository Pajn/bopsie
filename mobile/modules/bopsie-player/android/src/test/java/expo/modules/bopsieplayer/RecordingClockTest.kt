package expo.modules.bopsieplayer

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RecordingClockTest {
  @Test
  fun `frame zero is when each timestamp says it was, by the median`() {
    val rate = 48_000
    // Frame 0 at 5 s; one stamp read late says 7 ms later.
    val stamps = listOf(
      0L to 5_000_000_000L,
      48_000L to 6_000_000_000L,
      96_000L to 7_007_000_000L,
      144_000L to 8_000_000_000L,
    )
    assertEquals(5_000_000_000L, RecordingClock.frameZeroNanos(stamps, rate))
    assertNull(RecordingClock.frameZeroNanos(emptyList(), rate))
  }

  @Test
  fun `samples are little-endian bytes`() {
    val recording = MicRecording(shortArrayOf(1, -2, 0x1234), 48_000, 0)
    assertArrayEquals(byteArrayOf(1, 0, -2, -1, 0x34, 0x12), recording.bytes())
  }
}
