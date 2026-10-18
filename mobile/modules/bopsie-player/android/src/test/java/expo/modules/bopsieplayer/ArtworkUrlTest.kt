package expo.modules.bopsieplayer

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ArtworkUrlTest {
  @Test fun `moves image proxy URLs onto the connected origin`() {
    assertEquals(
      "https://music.example.com/ma/imageproxy/abc?size=512&fmt=jpeg",
      ArtworkUrl.reachable("http://172.30.0.5:8095/imageproxy/abc?size=512&fmt=jpeg", "https://music.example.com/ma/"),
    )
  }

  @Test fun `leaves other artwork alone`() {
    val remote = "https://i.scdn.co/image/abc"
    assertEquals(remote, ArtworkUrl.reachable(remote, "http://10.0.0.2:8095"))
    assertNull(ArtworkUrl.reachable(null, "http://10.0.0.2:8095"))
  }
}
