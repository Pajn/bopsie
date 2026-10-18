package expo.modules.bopsieplayer

import org.json.JSONObject

/**
 * What is playing, from the Sendspin `metadata` role. The server sends only
 * the fields that changed, so each update is merged into the last.
 */
data class NowPlaying(
  val title: String? = null,
  val artist: String? = null,
  val album: String? = null,
  val artworkUrl: String? = null,
  /** Milliseconds into the track at `progressServerTimeUs`. */
  val progressMs: Long? = null,
  val durationMs: Long? = null,
  /** Playback speed × 1000; 0 while paused. */
  val speed: Int = 0,
  val progressServerTimeUs: Long = 0,
) {
  fun merge(update: JSONObject): NowPlaying {
    var next = this
    if (update.has("title")) next = next.copy(title = update.optStringOrNull("title"))
    if (update.has("artist")) next = next.copy(artist = update.optStringOrNull("artist"))
    if (update.has("album")) next = next.copy(album = update.optStringOrNull("album"))
    if (update.has("artwork_url")) next = next.copy(artworkUrl = update.optStringOrNull("artwork_url"))
    if (update.has("progress")) {
      val progress = update.optJSONObject("progress")
      next = if (progress == null) {
        next.copy(progressMs = null, durationMs = null, speed = 0)
      } else {
        next.copy(
          progressMs = progress.getLong("track_progress"),
          durationMs = progress.getLong("track_duration").takeIf { it > 0 },
          speed = progress.getInt("playback_speed"),
          progressServerTimeUs = update.optLong("timestamp", progressServerTimeUs),
        )
      }
    }
    return next
  }

  /** Position at a server time, advanced at the reported speed and kept within the track. */
  fun positionMs(serverTimeUs: Long): Long {
    val base = progressMs ?: return 0
    val elapsedMs = (serverTimeUs - progressServerTimeUs) / 1000 * speed / 1000
    val position = base + maxOf(0, elapsedMs)
    return durationMs?.let { minOf(position, it) } ?: position
  }
}
