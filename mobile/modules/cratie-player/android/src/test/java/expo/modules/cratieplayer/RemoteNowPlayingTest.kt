package expo.modules.cratieplayer

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RemoteNowPlayingTest {
  private val json = """
    {"title":"Algorism","artist":"BABYMETAL · Bedroom","album":"METAL FORTH",
     "artworkUrl":null,"durationMs":217000,"positionMs":30000,"positionAt":1000000,
     "playing":true,"volume":42}
  """

  @Test fun `reads what the app hands over, with what it leaves out as null`() {
    val remote = RemoteNowPlaying.parse(json)
    assertEquals("Algorism", remote.title)
    assertEquals("METAL FORTH", remote.album)
    assertNull(remote.artworkUrl)
    assertEquals(217_000L, remote.durationMs)
    assertEquals(42, remote.volume)
  }

  @Test fun `advances the position while playing, within the track`() {
    val remote = RemoteNowPlaying.parse(json)
    assertEquals(35_000L, remote.positionMs(1_005_000))
    assertEquals(217_000L, remote.positionMs(9_000_000))
    // A clock behind the server's does not move it back.
    assertEquals(30_000L, remote.positionMs(990_000))
  }

  @Test fun `holds the position while paused`() {
    val remote = RemoteNowPlaying.parse(json).copy(playing = false)
    assertEquals(30_000L, remote.positionMs(1_005_000))
  }

  @Test fun `counts the volume in steps of five`() {
    assertEquals(8, RemoteNowPlaying.parse(json).deviceVolume)
    assertEquals(45, RemoteNowPlaying.volumeOf(9))
    assertEquals(100, RemoteNowPlaying.volumeOf(25))
    assertNull(RemoteNowPlaying.parse(json.replace("\"volume\":42", "\"volume\":null")).deviceVolume)
  }
}
