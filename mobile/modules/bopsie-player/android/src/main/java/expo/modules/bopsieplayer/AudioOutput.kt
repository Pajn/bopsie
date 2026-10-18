package expo.modules.bopsieplayer

import android.media.AudioAttributes
import android.media.AudioTimestamp
import android.media.AudioTrack
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import kotlin.math.pow
import android.media.AudioFormat as AndroidAudioFormat

/**
 * Plays PCM chunks through an AudioTrack at the times the server set. A
 * writer thread takes chunks in order, asks the scheduler how to line each up
 * with the output, and writes it. When a stream is open but the output is
 * about to run dry, it writes silence, so the output timeline never stalls
 * and the next chunk is measured against the true clock.
 *
 * @param clientTimeOf converts a server time to local monotonic microseconds,
 *   or null while the clocks are not synchronized.
 */
class AudioOutput(private val clientTimeOf: (Long) -> Long?) {
  private val queue = LinkedBlockingQueue<Pair<AudioChunk, Int>>()
  private val lock = Object()
  private var track: AudioTrack? = null
  private var format: AudioFormat? = null
  private var framesWritten = 0L
  private var aligned = false
  /** Bumped by every start, clear and stop, so a write in progress knows to abandon its chunk. */
  private var generation = 0
  private var gain = 1f
  @Volatile private var streaming = false
  @Volatile private var running = true
  private val timestamp = AudioTimestamp()

  private val writer = Thread({ writeLoop() }, "bopsie-audio").apply { start() }

  fun start(newFormat: AudioFormat) {
    synchronized(lock) {
      if (newFormat != format || track == null) {
        track?.release()
        track = createTrack(newFormat)
        format = newFormat
        framesWritten = 0
      }
      aligned = false
      generation++
      streaming = true
      track?.play()
    }
  }

  fun enqueue(chunk: AudioChunk) {
    synchronized(lock) { if (streaming) queue.offer(chunk to generation) }
  }

  /** Drops queued and buffered audio but keeps the stream open for what follows. */
  fun clear() {
    synchronized(lock) {
      queue.clear()
      track?.let { it.pause(); it.flush(); framesWritten = 0; if (streaming) it.play() }
      aligned = false
      generation++
    }
  }

  fun stop() {
    synchronized(lock) {
      streaming = false
      queue.clear()
      track?.let { it.pause(); it.flush() }
      framesWritten = 0
      aligned = false
      generation++
    }
  }

  /** Volume 0–100 is perceived loudness; Sendspin maps it to gain as `(volume/100)^1.5`. */
  fun setVolume(volume: Int, muted: Boolean) {
    synchronized(lock) {
      gain = if (muted) 0f else (volume.coerceIn(0, 100) / 100.0).pow(1.5).toFloat()
      track?.setVolume(gain)
    }
  }

  fun release() {
    running = false
    writer.interrupt()
    synchronized(lock) {
      track?.release()
      track = null
      streaming = false
    }
  }

  private fun createTrack(format: AudioFormat): AudioTrack {
    val channelMask = if (format.channels == 1) AndroidAudioFormat.CHANNEL_OUT_MONO else AndroidAudioFormat.CHANNEL_OUT_STEREO
    val encoding = AndroidAudioFormat.ENCODING_PCM_16BIT
    val minimum = AudioTrack.getMinBufferSize(format.sampleRate, channelMask, encoding)
    // A tenth of a second of headroom keeps a busy device from underrunning.
    val bufferBytes = maxOf(minimum * 2, format.sampleRate / 10 * format.bytesPerFrame)
    return AudioTrack.Builder()
      .setAudioAttributes(AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_MEDIA)
        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
        .build())
      .setAudioFormat(AndroidAudioFormat.Builder()
        .setSampleRate(format.sampleRate)
        .setChannelMask(channelMask)
        .setEncoding(encoding)
        .build())
      .setBufferSizeInBytes(bufferBytes)
      .setTransferMode(AudioTrack.MODE_STREAM)
      .build()
      .also { it.setVolume(gain) }
  }

  private fun writeLoop() {
    while (running) {
      val next = try { queue.poll(10, TimeUnit.MILLISECONDS) } catch (_: InterruptedException) { null }
      synchronized(lock) {
        val track = track ?: return@synchronized
        val format = format ?: return@synchronized
        if (!streaming) return@synchronized
        if (next == null) {
          val buffered = cursorUs(track, format) - System.nanoTime() / 1000
          if (aligned && buffered < UNDERRUN_GUARD_US) writeSilence(track, format, format.sampleRate / 100, generation)
          return@synchronized
        }
        val (chunk, gen) = next
        // Queued before a clear: no longer wanted.
        if (gen != generation) return@synchronized
        val local = clientTimeOf(chunk.serverTimeUs) ?: return@synchronized
        val frames = chunk.length / format.bytesPerFrame
        val alignment = Scheduler.align(local, cursorUs(track, format), frames, format.sampleRate, snap = !aligned)
        aligned = true
        if (alignment.dropFrames >= frames) return@synchronized
        val start = chunk.offset + alignment.dropFrames * format.bytesPerFrame
        if (!writeSilence(track, format, alignment.silenceFrames, gen)) return@synchronized
        repeat(alignment.repeatFrames) { write(track, format, chunk.data, start, format.bytesPerFrame, gen) }
        write(track, format, chunk.data, start, chunk.data.size - start, gen)
      }
    }
  }

  /** When the next frame written will be heard, in local monotonic microseconds. */
  private fun cursorUs(track: AudioTrack, format: AudioFormat): Long {
    val rate = format.sampleRate
    return if (track.getTimestamp(timestamp) && timestamp.framePosition > 0) {
      timestamp.nanoTime / 1000 + (framesWritten - timestamp.framePosition) * 1_000_000 / rate
    } else {
      // No hardware timestamp yet: assume what is buffered plays from now.
      val played = track.playbackHeadPosition.toLong() and 0xFFFFFFFFL
      System.nanoTime() / 1000 + (framesWritten - played) * 1_000_000 / rate
    }
  }

  private fun writeSilence(track: AudioTrack, format: AudioFormat, frames: Int, gen: Int): Boolean {
    if (frames <= 0) return true
    return write(track, format, ByteArray(frames * format.bytesPerFrame), 0, frames * format.bytesPerFrame, gen)
  }

  /** Writes all of `length` unless the stream is started, cleared or stopped meanwhile, and says whether it did. */
  private fun write(track: AudioTrack, format: AudioFormat, data: ByteArray, offset: Int, length: Int, gen: Int): Boolean =
    PcmWrite.writeAll(
      lock, data, offset, length,
      wanted = { running && generation == gen },
      write = { bytes, from, count -> track.write(bytes, from, count, AudioTrack.WRITE_NON_BLOCKING) },
      written = { bytes -> framesWritten += bytes / format.bytesPerFrame },
    )

  companion object {
    /** Less audio than this left in the output counts as running dry. */
    private const val UNDERRUN_GUARD_US = 30_000L
  }
}
