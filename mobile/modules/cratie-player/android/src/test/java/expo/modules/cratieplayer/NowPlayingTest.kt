package expo.modules.cratieplayer

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class NowPlayingTest {
  @Test fun `merges partial updates and clears fields sent as null`() {
    val first = NowPlaying().merge(JSONObject("""{"timestamp":0,"title":"One","artist":"A","album":"X"}"""))
    val second = first.merge(JSONObject("""{"timestamp":5,"title":"Two","album":null}"""))
    assertEquals("Two", second.title)
    assertEquals("A", second.artist)
    assertNull(second.album)
  }

  @Test fun `advances progress at the reported speed and stops at the end`() {
    val playing = NowPlaying().merge(JSONObject(
      """{"timestamp":1000000,"progress":{"track_progress":10000,"track_duration":12000,"playback_speed":1000}}""",
    ))
    assertEquals(10_000, playing.positionMs(1_000_000))
    assertEquals(11_500, playing.positionMs(2_500_000))
    assertEquals(12_000, playing.positionMs(9_000_000))

    val paused = playing.merge(JSONObject(
      """{"timestamp":3000000,"progress":{"track_progress":11000,"track_duration":12000,"playback_speed":0}}""",
    ))
    assertEquals(11_000, paused.positionMs(8_000_000))
  }
}
