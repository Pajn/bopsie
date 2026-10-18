package expo.modules.bopsieplayer

import org.json.JSONObject
import kotlin.math.roundToInt

/**
 * What the app controls on another player, which the phone's media session shows while the
 * phone itself plays nothing: the app follows that player through the server's API and
 * hands its state over, and the session's commands go back to the app to send.
 */
data class RemoteNowPlaying(
  val title: String,
  val artist: String?,
  val album: String?,
  val artworkUrl: String?,
  val durationMs: Long?,
  /** Milliseconds into the track at `positionAtMs`. */
  val positionMs: Long,
  /** Unix milliseconds. */
  val positionAtMs: Long,
  val playing: Boolean,
  /** 0–100, or null where the player has no volume control. */
  val volume: Int?,
) {
  /** Position at `nowMs`, advanced while playing and kept within the track. */
  fun positionMs(nowMs: Long): Long {
    val position = positionMs + if (playing) maxOf(0, nowMs - positionAtMs) else 0
    return durationMs?.let { minOf(position, it) } ?: position
  }

  /** The volume in the session's steps. */
  val deviceVolume: Int? get() = volume?.let { (it / STEP.toFloat()).roundToInt() }

  companion object {
    /** Each press of a volume key moves the volume this much, out of 100. */
    const val STEP = 5
    const val STEPS = 100 / STEP

    /** The volume, out of 100, for a number of the session's steps. */
    fun volumeOf(deviceVolume: Int): Int = (deviceVolume * STEP).coerceIn(0, 100)

    fun parse(json: String): RemoteNowPlaying {
      val o = JSONObject(json)
      return RemoteNowPlaying(
        title = o.getString("title"),
        artist = o.optStringOrNull("artist"),
        album = o.optStringOrNull("album"),
        artworkUrl = o.optStringOrNull("artworkUrl"),
        durationMs = if (o.isNull("durationMs")) null else o.getLong("durationMs"),
        positionMs = o.optLong("positionMs", 0),
        positionAtMs = o.optLong("positionAt", 0),
        playing = o.optBoolean("playing", false),
        volume = if (o.isNull("volume")) null else o.getInt("volume"),
      )
    }
  }
}
