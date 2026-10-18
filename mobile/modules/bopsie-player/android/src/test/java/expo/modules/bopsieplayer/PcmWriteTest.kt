package expo.modules.bopsieplayer

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.ByteArrayOutputStream

class PcmWriteTest {
  private val lock = Object()
  private val data = ByteArray(10) { it.toByte() }

  /** Writes under the lock, as the output's writer thread does. */
  private fun writeAll(
    wanted: () -> Boolean = { true },
    written: (Int) -> Unit = {},
    write: (ByteArray, Int, Int) -> Int,
  ) = synchronized(lock) { PcmWrite.writeAll(lock, data, 2, 8, wanted, write, written) }

  @Test fun `writes all of it a little at a time while the output has room for little`() {
    val out = ByteArrayOutputStream()
    val amounts = mutableListOf<Int>()
    val done = writeAll(written = { amounts += it }) { bytes, offset, length ->
      val taken = minOf(3, length)
      out.write(bytes, offset, taken)
      taken
    }
    assertTrue(done)
    assertArrayEquals(data.copyOfRange(2, 10), out.toByteArray())
    assertEquals(listOf(3, 3, 2), amounts)
  }

  @Test fun `gives up once the audio is no longer wanted`() {
    var calls = 0
    val done = writeAll(wanted = { calls == 0 }) { _, _, _ -> calls++; 1 }
    assertFalse(done)
    assertEquals(1, calls)
  }

  @Test fun `gives up when the output fails`() {
    assertFalse(writeAll { _, _, _ -> -1 })
  }

  @Test fun `gives up, rather than throwing, when interrupted while the output is full`() {
    var done: Boolean? = null
    var thrown: Throwable? = null
    val writer = Thread {
      try {
        done = writeAll { _, _, _ -> 0 }
      } catch (e: Throwable) {
        thrown = e
      }
    }
    writer.start()
    // Interrupt it while it waits for room, as releasing the output does.
    while (writer.state != Thread.State.TIMED_WAITING) Thread.sleep(1)
    writer.interrupt()
    writer.join(1_000)

    assertFalse(writer.isAlive)
    assertNull(thrown)
    assertEquals(false, done)
  }
}
