package expo.modules.bopsieplayer

import android.annotation.SuppressLint
import android.content.Context
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTimestamp
import android.media.MediaRecorder

/** A recording from the microphone, as 16-bit mono samples, and when its first one was heard. */
class MicRecording(val samples: ShortArray, val sampleRate: Int, val firstFrameNanos: Long) {
  /** The samples as little-endian bytes, as JS reads them into an Int16Array. */
  fun bytes(): ByteArray {
    val out = ByteArray(samples.size * 2)
    for (i in samples.indices) {
      val sample = samples[i].toInt()
      out[2 * i] = sample.toByte()
      out[2 * i + 1] = (sample shr 8).toByte()
    }
    return out
  }
}

/** When frame 0 of a recording was heard, from timestamps that each pair a frame with when it was heard. */
object RecordingClock {
  /** The median of what each timestamp says, so that one taken late does not move it. Null without any. */
  fun frameZeroNanos(stamps: List<Pair<Long, Long>>, sampleRate: Int): Long? {
    if (stamps.isEmpty()) return null
    val estimates = stamps.map { (frame, nanos) -> nanos - frame * 1_000_000_000L / sampleRate }.sorted()
    return estimates[estimates.size / 2]
  }
}

/**
 * Records the microphone for calibrating speakers: unprocessed where the device offers it, so that no
 * noise suppression or gain control moves or swallows a chirp. Each sample's time is read from the
 * recorder's own timestamps, on the monotonic clock the Sendspin clock filter is kept on.
 */
object MicRecorder {
  private const val SAMPLE_RATE = 48_000

  /** Records for [durationMs], blocking. Needs the RECORD_AUDIO permission. */
  @SuppressLint("MissingPermission")
  fun record(context: Context, durationMs: Int): MicRecording {
    val audioManager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    val source =
      if (audioManager.getProperty(AudioManager.PROPERTY_SUPPORT_AUDIO_SOURCE_UNPROCESSED) == "true") {
        MediaRecorder.AudioSource.UNPROCESSED
      } else {
        MediaRecorder.AudioSource.VOICE_RECOGNITION
      }
    val minimum = AudioRecord.getMinBufferSize(SAMPLE_RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
    require(minimum > 0) { "The microphone cannot record at $SAMPLE_RATE Hz" }
    val recorder = AudioRecord(
      source,
      SAMPLE_RATE,
      AudioFormat.CHANNEL_IN_MONO,
      AudioFormat.ENCODING_PCM_16BIT,
      maxOf(minimum, SAMPLE_RATE / 5 * 2),
    )
    check(recorder.state == AudioRecord.STATE_INITIALIZED) { "The microphone is not available" }
    val total = (SAMPLE_RATE.toLong() * durationMs / 1000).toInt()
    val samples = ShortArray(total)
    val stamps = mutableListOf<Pair<Long, Long>>()
    val timestamp = AudioTimestamp()
    var read = 0
    var fallback: Long? = null
    try {
      recorder.startRecording()
      while (read < total) {
        val count = recorder.read(samples, read, minOf(SAMPLE_RATE / 50, total - read))
        check(count >= 0) { "The microphone stopped: $count" }
        if (fallback == null && count > 0) {
          // What the first read says, if the recorder keeps no timestamps: it arrived as it was read.
          fallback = System.nanoTime() - count * 1_000_000_000L / SAMPLE_RATE
        }
        read += count
        if (recorder.getTimestamp(timestamp, AudioTimestamp.TIMEBASE_MONOTONIC) == AudioRecord.SUCCESS) {
          stamps.add(timestamp.framePosition to timestamp.nanoTime)
        }
      }
    } finally {
      recorder.stop()
      recorder.release()
    }
    val first = RecordingClock.frameZeroNanos(stamps, SAMPLE_RATE)
      ?: checkNotNull(fallback) { "The microphone recorded nothing" }
    return MicRecording(samples, SAMPLE_RATE, first)
  }
}
