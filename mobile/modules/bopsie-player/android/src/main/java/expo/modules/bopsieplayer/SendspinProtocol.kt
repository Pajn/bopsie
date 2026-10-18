package expo.modules.bopsieplayer

import org.json.JSONArray
import org.json.JSONObject
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** PCM layout of an audio stream, from `stream/start`. */
data class AudioFormat(val codec: String, val sampleRate: Int, val channels: Int, val bitDepth: Int) {
  val bytesPerFrame: Int get() = channels * (bitDepth / 8)
}

/** One binary audio chunk: when its first sample must be heard, on the server clock, and its bytes. */
class AudioChunk(val serverTimeUs: Long, val data: ByteArray, val offset: Int) {
  val length: Int get() = data.size - offset
}

sealed class ServerMessage {
  object AuthOk : ServerMessage()
  data class Hello(val activeRoles: List<String>) : ServerMessage()
  data class Time(val clientTransmitted: Long, val serverReceived: Long, val serverTransmitted: Long) : ServerMessage()
  data class StreamStart(val format: AudioFormat?) : ServerMessage()
  object StreamClear : ServerMessage()
  object StreamEnd : ServerMessage()
  data class State(val metadata: JSONObject?, val controller: JSONObject?) : ServerMessage()
  data class GroupUpdate(val playbackState: String?) : ServerMessage()
  data class Command(val command: String, val volume: Int?, val mute: Boolean?) : ServerMessage()
  data class Unknown(val type: String) : ServerMessage()
}

/**
 * Sendspin's unencrypted wire as the server speaks it on `/sendspin`. JSON
 * messages are `{type, payload}` text frames; audio arrives in binary frames.
 */
object SendspinProtocol {
  const val AUDIO_CHUNK = 4
  private const val AUDIO_HEADER_BYTES = 9

  /** The server registers only these products as players that can be played to. */
  private const val PRODUCT_NAME = "Mobile Application"

  private fun message(type: String, payload: JSONObject) =
    JSONObject().put("type", type).put("payload", payload).toString()

  /** First frame on the `/sendspin` route of the API server; not wrapped in a payload. */
  fun auth(token: String, clientId: String): String =
    JSONObject().put("type", "auth").put("token", token).put("client_id", clientId).toString()

  fun hello(clientId: String, name: String, softwareVersion: String, formats: List<AudioFormat>, bufferCapacity: Int): String {
    val support = JSONObject()
      .put("supported_formats", JSONArray(formats.map {
        JSONObject().put("codec", it.codec).put("channels", it.channels).put("sample_rate", it.sampleRate).put("bit_depth", it.bitDepth)
      }))
      .put("buffer_capacity", bufferCapacity)
      .put("supported_commands", JSONArray(listOf("volume", "mute")))
    return message("client/hello", JSONObject()
      .put("client_id", clientId)
      .put("name", name)
      .put("version", 1)
      .put("supported_roles", JSONArray(listOf("player@v1", "controller@v1", "metadata@v1")))
      .put("device_info", JSONObject()
        .put("product_name", PRODUCT_NAME)
        .put("manufacturer", "Bopsie")
        .put("software_version", softwareVersion))
      .put("player@v1_support", support))
  }

  /** The player's state: whether it can play now, and its volume and timing. */
  fun state(available: Boolean, volume: Int, muted: Boolean, leadTimeMs: Int): String =
    message("client/state", JSONObject()
      .put("available", available)
      .put("player", JSONObject()
        .put("volume", volume)
        .put("muted", muted)
        .put("static_delay_ms", 0)
        .put("required_lead_time_ms", leadTimeMs)
        .put("min_buffer_ms", leadTimeMs)))

  fun time(clientTransmittedUs: Long): String =
    message("client/time", JSONObject().put("client_transmitted", clientTransmittedUs))

  /** A transport command for the group this player is in, such as `play`, `pause` or `next`. */
  fun command(command: String, positionMs: Long? = null): String {
    val controller = JSONObject().put("command", command)
    if (positionMs != null) controller.put("position_ms", positionMs)
    return message("client/command", JSONObject().put("controller", controller))
  }

  fun goodbye(reason: String): String = message("client/goodbye", JSONObject().put("reason", reason))

  fun parse(text: String): ServerMessage {
    val json = JSONObject(text)
    val type = json.optString("type")
    val payload = json.optJSONObject("payload") ?: JSONObject()
    return when (type) {
      "auth_ok" -> ServerMessage.AuthOk
      "server/hello" -> ServerMessage.Hello(payload.optJSONArray("active_roles").strings())
      "server/time" -> ServerMessage.Time(
        payload.getLong("client_transmitted"),
        payload.getLong("server_received"),
        payload.getLong("server_transmitted"),
      )
      "stream/start" -> ServerMessage.StreamStart(payload.optJSONObject("player")?.let {
        AudioFormat(it.getString("codec"), it.getInt("sample_rate"), it.getInt("channels"), it.optInt("bit_depth", 16))
      })
      "stream/clear" -> if (payload.affectsPlayer()) ServerMessage.StreamClear else ServerMessage.Unknown(type)
      "stream/end" -> if (payload.affectsPlayer()) ServerMessage.StreamEnd else ServerMessage.Unknown(type)
      "server/state" -> ServerMessage.State(payload.optJSONObject("metadata"), payload.optJSONObject("controller"))
      "group/update" -> ServerMessage.GroupUpdate(payload.optStringOrNull("playback_state"))
      "server/command" -> payload.optJSONObject("player")?.let {
        ServerMessage.Command(
          it.optString("command"),
          if (it.has("volume")) it.getInt("volume") else null,
          if (it.has("mute")) it.getBoolean("mute") else null,
        )
      } ?: ServerMessage.Unknown(type)
      else -> ServerMessage.Unknown(type)
    }
  }

  /** An audio chunk, or null for other binary messages such as artwork. */
  fun parseBinary(bytes: ByteArray): AudioChunk? {
    if (bytes.size < AUDIO_HEADER_BYTES || bytes[0].toInt() != AUDIO_CHUNK) return null
    val timestamp = ByteBuffer.wrap(bytes, 1, 8).order(ByteOrder.BIG_ENDIAN).long
    return AudioChunk(timestamp, bytes, AUDIO_HEADER_BYTES)
  }

  /** `roles` lists the roles a clear or end applies to; absent means all of them. */
  private fun JSONObject.affectsPlayer(): Boolean {
    val roles = optJSONArray("roles") ?: return true
    return roles.strings().any { it == "player" || it.startsWith("player@") }
  }

  private fun JSONArray?.strings(): List<String> =
    if (this == null) emptyList() else (0 until length()).map { getString(it) }
}

internal fun JSONObject.optStringOrNull(key: String): String? =
  if (has(key) && !isNull(key)) getString(key) else null
