package expo.modules.cratieplayer

import kotlin.math.abs

/**
 * How to line up a chunk with the output: frames to drop from its start,
 * silence to write before it, and how many of its first frame to repeat.
 */
data class Alignment(val dropFrames: Int = 0, val silenceFrames: Int = 0, val repeatFrames: Int = 0)

/**
 * Keeps audio on the server's timeline. Each chunk names the local time its
 * first sample must be heard; the cursor is when the next frame written to
 * the output will be heard. Big gaps, at a stream's start or after an
 * underrun, are closed in one step with silence or by dropping late audio.
 * Small ones are corrected by dropping or repeating single frames, at most
 * 0.5 % of a chunk, which Sendspin's synchronization guidance allows and
 * which is inaudible.
 */
object Scheduler {
  /** Beyond this, correct in one step. */
  const val SNAP_THRESHOLD_US = 20_000L
  /** Within this, the chunk plays as it is. */
  const val TOLERANCE_US = 100L

  fun align(chunkStartUs: Long, cursorUs: Long, frames: Int, sampleRate: Int, snap: Boolean): Alignment {
    val errorUs = chunkStartUs - cursorUs
    val errorFrames = errorUs * sampleRate / 1_000_000
    return when {
      snap || abs(errorUs) > SNAP_THRESHOLD_US ->
        if (errorFrames >= 0) Alignment(silenceFrames = errorFrames.toInt())
        else Alignment(dropFrames = minOf(frames.toLong(), -errorFrames).toInt())
      abs(errorUs) <= TOLERANCE_US -> Alignment()
      else -> {
        val correction = minOf(abs(errorFrames), maxOf(1, frames / 200).toLong()).coerceAtLeast(1).toInt()
        if (errorUs > 0) Alignment(repeatFrames = correction) else Alignment(dropFrames = minOf(correction, frames))
      }
    }
  }
}
