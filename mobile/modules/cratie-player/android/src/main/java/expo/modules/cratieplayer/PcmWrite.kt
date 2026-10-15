package expo.modules.cratieplayer

/** Writing audio to an output that takes only what it has room for, such as a non-blocking AudioTrack. */
internal object PcmWrite {
  /**
   * Writes all of `length` bytes from `offset` unless the audio stops being `wanted`, the
   * output fails, or the thread is interrupted meanwhile, and says whether it did. `write`
   * takes what it can and says how much, or a negative number on failure; `written` hears
   * each amount taken. The caller holds `lock`: while the output is full this waits on it,
   * which releases it, so control calls are never held up.
   */
  fun writeAll(
    lock: Object,
    data: ByteArray,
    offset: Int,
    length: Int,
    wanted: () -> Boolean,
    write: (ByteArray, Int, Int) -> Int,
    written: (Int) -> Unit,
  ): Boolean {
    var position = offset
    val end = offset + length
    while (position < end) {
      if (!wanted()) return false
      val taken = write(data, position, end - position)
      if (taken < 0) return false
      position += taken
      written(taken)
      if (position < end) {
        try {
          lock.wait(5)
        } catch (_: InterruptedException) {
          // Releasing the output interrupts the writer: stop, and leave the interrupt for it to see.
          Thread.currentThread().interrupt()
          return false
        }
      }
    }
    return true
  }
}
