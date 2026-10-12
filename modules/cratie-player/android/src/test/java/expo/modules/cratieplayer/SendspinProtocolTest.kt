package expo.modules.cratieplayer

import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SendspinProtocolTest {
  @Test fun `hello registers a mobile application player under both support keys`() {
    val pcm = AudioFormat("pcm", 48_000, 2, 16)
    val hello = JSONObject(SendspinProtocol.hello("id-1", "Phone", "0.1.0", listOf(pcm), 1_048_576))
    assertEquals("client/hello", hello.getString("type"))
    val payload = hello.getJSONObject("payload")
    assertEquals("id-1", payload.getString("client_id"))
    assertEquals(1, payload.getInt("version"))
    assertEquals("Mobile Application", payload.getJSONObject("device_info").getString("product_name"))
    for (key in listOf("player@v1_support", "player_support")) {
      val format = payload.getJSONObject(key).getJSONArray("supported_formats").getJSONObject(0)
      assertEquals("pcm", format.getString("codec"))
      assertEquals(48_000, format.getInt("sample_rate"))
    }
  }

  @Test fun `state carries both the 2_10 and the older availability fields`() {
    val payload = JSONObject(SendspinProtocol.state(true, 40, false, 200)).getJSONObject("payload")
    assertTrue(payload.getBoolean("available"))
    assertEquals("synchronized", payload.getString("state"))
    assertEquals(40, payload.getJSONObject("player").getInt("volume"))
  }

  @Test fun `parses server messages`() {
    assertEquals(ServerMessage.AuthOk, SendspinProtocol.parse("""{"type":"auth_ok"}"""))
    assertEquals(
      ServerMessage.Time(1, 2, 3),
      SendspinProtocol.parse("""{"type":"server/time","payload":{"client_transmitted":1,"server_received":2,"server_transmitted":3}}"""),
    )
    assertEquals(
      ServerMessage.StreamStart(AudioFormat("pcm", 44_100, 2, 16)),
      SendspinProtocol.parse("""{"type":"stream/start","payload":{"server_transmitted":9,"player":{"codec":"pcm","sample_rate":44100,"channels":2,"bit_depth":16}}}"""),
    )
    assertEquals(
      ServerMessage.Command("volume", 70, null),
      SendspinProtocol.parse("""{"type":"server/command","payload":{"player":{"command":"volume","volume":70}}}"""),
    )
    assertEquals(ServerMessage.Unknown("future/thing"), SendspinProtocol.parse("""{"type":"future/thing","payload":{}}"""))
  }

  @Test fun `a clear for other roles leaves the player alone`() {
    assertEquals(ServerMessage.StreamClear, SendspinProtocol.parse("""{"type":"stream/clear","payload":{}}"""))
    assertEquals(ServerMessage.StreamEnd, SendspinProtocol.parse("""{"type":"stream/end","payload":{"roles":["player"]}}"""))
    assertTrue(SendspinProtocol.parse("""{"type":"stream/end","payload":{"roles":["visualizer"]}}""") is ServerMessage.Unknown)
  }

  @Test fun `reads the audio timestamp big-endian`() {
    val frame = byteArrayOf(4, 0, 0, 0, 0, 0, 0x01, 0x02, 0x03, 9, 8, 7)
    val chunk = SendspinProtocol.parseBinary(frame)!!
    assertEquals(0x010203L, chunk.serverTimeUs)
    assertEquals(3, chunk.length)
    assertArrayEquals(byteArrayOf(9, 8, 7), frame.copyOfRange(chunk.offset, frame.size))
  }

  @Test fun `ignores binary messages that are not audio`() {
    assertNull(SendspinProtocol.parseBinary(byteArrayOf(8, 0, 0, 0, 0, 0, 0, 0, 0, 1)))
    assertNull(SendspinProtocol.parseBinary(byteArrayOf(4, 0)))
  }
}
