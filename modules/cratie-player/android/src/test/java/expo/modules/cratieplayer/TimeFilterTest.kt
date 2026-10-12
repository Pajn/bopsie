package expo.modules.cratieplayer

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs
import kotlin.random.Random

class TimeFilterTest {
  /** A server clock running `offset` µs ahead of ours and drifting by `drift` (µs per µs). */
  private fun measurements(offset: Long, drift: Double, jitterUs: Int, seed: Int = 1): Sequence<Triple<Long, Long, Long>> {
    val random = Random(seed)
    return generateSequence(1_000_000L) { it + 1_000_000L }.map { t1 ->
      val there = 2_000L + random.nextInt(jitterUs + 1)
      val back = 2_000L + random.nextInt(jitterUs + 1)
      val trueOffset = offset + (drift * t1).toLong()
      val t2 = t1 + there + trueOffset
      val t3 = t2 + 50
      val t4 = t1 + there + 50 + back
      val measurement = ((t2 - t1) + (t3 - t4)) / 2
      val maxError = ((t4 - t1) - (t3 - t2)) / 2
      Triple(measurement, maxError, t4)
    }
  }

  @Test fun `is not synchronized until two measurements arrive`() {
    val filter = TimeFilter()
    assertFalse(filter.isSynchronized)
    filter.update(500, 100, 1_000)
    assertFalse(filter.isSynchronized)
    filter.update(500, 100, 2_000)
    assertTrue(filter.isSynchronized)
  }

  @Test fun `converges on a fixed offset despite jitter`() {
    val filter = TimeFilter()
    measurements(offset = 1_234_567_890L, drift = 0.0, jitterUs = 400).take(60)
      .forEach { (m, e, t) -> filter.update(m, e, t) }
    val client = 70_000_000L
    assertTrue(abs(filter.serverTime(client) - (client + 1_234_567_890L)) < 300)
  }

  @Test fun `tracks drift between the clocks`() {
    val filter = TimeFilter()
    val drift = 50e-6 // 50 ppm, a cheap crystal
    measurements(offset = 10_000L, drift = drift, jitterUs = 100).take(200)
      .forEach { (m, e, t) -> filter.update(m, e, t) }
    val client = 250_000_000L
    val expected = client + 10_000L + (drift * client).toLong()
    assertTrue("off by ${filter.serverTime(client) - expected}", abs(filter.serverTime(client) - expected) < 500)
  }

  @Test fun `client and server conversions invert each other`() {
    val filter = TimeFilter()
    measurements(offset = -5_000_000L, drift = 20e-6, jitterUs = 50).take(150)
      .forEach { (m, e, t) -> filter.update(m, e, t) }
    val client = 123_456_789L
    assertEquals(client.toDouble(), filter.clientTime(filter.serverTime(client)).toDouble(), 1.0)
  }

  @Test fun `ignores measurements that go back in time`() {
    val filter = TimeFilter()
    filter.update(100, 10, 5_000)
    filter.update(900, 10, 4_000)
    assertEquals(1, filter.count)
  }
}
