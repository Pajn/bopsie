package expo.modules.cratieplayer

import android.os.Handler
import android.os.HandlerThread
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import java.util.concurrent.TimeUnit

/** Microseconds on the monotonic clock AudioTrack timestamps also use. */
fun monotonicUs(): Long = System.nanoTime() / 1000

/**
 * A Sendspin player connection through the server's `/sendspin` route.
 *
 * Authenticates with the account token, says hello as a player, controller
 * and metadata client, synchronizes clocks, and only then reports itself
 * available, so the server never streams to a player that cannot schedule
 * audio. Control flow runs on one handler thread; audio chunks are handed to
 * the listener straight from the socket thread.
 */
class SendspinClient(private val config: Config, private val listener: Listener) {
  data class Config(
    /** `ws(s)://…/sendspin` on the server's API origin. */
    val url: String,
    val token: String,
    /** Persisted per install; becomes the server's player and queue id. */
    val clientId: String,
    val name: String,
    val softwareVersion: String,
  )

  enum class Status { CONNECTING, CONNECTED, RECONNECTING, UNAUTHORIZED, CLOSED }

  interface Listener {
    fun onStatus(status: Status, message: String?)
    fun onStreamStart(format: AudioFormat)
    fun onStreamClear()
    fun onStreamEnd()
    fun onAudio(chunk: AudioChunk)
    fun onNowPlaying(nowPlaying: NowPlaying)
    fun onPlaybackState(state: String)
    fun onVolume(volume: Int, muted: Boolean)
    fun onControllerCommands(commands: Set<String>)
  }

  private val thread = HandlerThread("cratie-sendspin").apply { start() }
  private val handler = Handler(thread.looper)
  private val http = OkHttpClient.Builder()
    .pingInterval(20, TimeUnit.SECONDS)
    .readTimeout(0, TimeUnit.MILLISECONDS)
    .build()
  private val filter = TimeFilter()

  private var socket: WebSocket? = null
  private var status = Status.CLOSED
  private var closing = false
  private var attempt = 0
  private var announcedAvailable = false
  private var nowPlaying = NowPlaying()
  private var volume = 100
  private var muted = false

  // The time-sync burst in progress: replies outstanding and the best sample so far.
  private var burstRemaining = 0
  private var burstBest: Triple<Long, Long, Long>? = null
  private var burstFeedsAll = false
  private val burstTimeout = Runnable { finishBurst() }
  private val nextBurst = Runnable { startBurst(feedAll = false) }

  fun connect() = handler.post {
    closing = false
    attempt = 0
    open()
  }

  /** Says goodbye so the server forgets the player rather than waiting for it to return. */
  fun close(reason: String = "user_request") = handler.post {
    closing = true
    handler.removeCallbacksAndMessages(null)
    socket?.let { it.send(SendspinProtocol.goodbye(reason)); it.close(1000, reason) }
    socket = null
    setStatus(Status.CLOSED, null)
  }

  fun release() {
    close("shutdown")
    handler.post { thread.quitSafely() }
  }

  /** A transport command for this player's group, such as `pause` or `next`. */
  fun command(command: String, positionMs: Long? = null) = handler.post {
    socket?.send(SendspinProtocol.command(command, positionMs))
  }

  /** Applies a volume set on the device and reports it to the server. */
  fun setVolume(newVolume: Int, newMuted: Boolean) = handler.post {
    volume = newVolume.coerceIn(0, 100)
    muted = newMuted
    listener.onVolume(volume, muted)
    sendState()
  }

  fun clientTimeOf(serverTimeUs: Long): Long? =
    synchronized(filter) { if (filter.isSynchronized) filter.clientTime(serverTimeUs) else null }

  /** When [clientTimeUs], on the monotonic clock, falls on the server's clock, once the clocks are in step. */
  fun serverTimeOf(clientTimeUs: Long): Long? =
    synchronized(filter) { if (filter.isSynchronized) filter.serverTime(clientTimeUs) else null }

  fun serverNowUs(): Long? =
    synchronized(filter) { if (filter.isSynchronized) filter.serverTime(monotonicUs()) else null }

  private fun open() {
    setStatus(if (attempt == 0) Status.CONNECTING else Status.RECONNECTING, null)
    announcedAvailable = false
    synchronized(filter) { filter.reset() }
    val request = Request.Builder().url(config.url).build()
    val current = http.newWebSocket(request, object : WebSocketListener() {
      override fun onOpen(webSocket: WebSocket, response: Response) {
        webSocket.send(SendspinProtocol.auth(config.token, config.clientId))
      }

      override fun onMessage(webSocket: WebSocket, text: String) {
        // T4 for clock sync is when the reply arrived, before any queueing.
        val receivedUs = monotonicUs()
        handler.post { if (socket === webSocket) receive(webSocket, text, receivedUs) }
      }

      override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
        if (socket !== webSocket) return
        SendspinProtocol.parseBinary(bytes.toByteArray())?.let(listener::onAudio)
      }

      override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
        webSocket.close(code, null)
        handler.post { if (socket === webSocket) dropped(code, reason) }
      }

      override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
        handler.post { if (socket === webSocket) dropped(null, t.message ?: "connection failed") }
      }
    })
    socket = current
  }

  private fun receive(webSocket: WebSocket, text: String, receivedUs: Long) {
    val message = try { SendspinProtocol.parse(text) } catch (_: Exception) { return }
    when (message) {
      ServerMessage.AuthOk -> webSocket.send(SendspinProtocol.hello(
        config.clientId, config.name, config.softwareVersion, SUPPORTED_FORMATS, BUFFER_CAPACITY,
      ))
      is ServerMessage.Hello -> {
        attempt = 0
        // Every sample of the first burst goes into the filter so it is
        // ready within a few round trips; later bursts feed only their best.
        startBurst(feedAll = true)
      }
      is ServerMessage.Time -> timeSample(message, receivedUs)
      is ServerMessage.StreamStart -> message.format?.let(listener::onStreamStart)
      ServerMessage.StreamClear -> listener.onStreamClear()
      ServerMessage.StreamEnd -> listener.onStreamEnd()
      is ServerMessage.State -> {
        message.metadata?.let { nowPlaying = nowPlaying.merge(it); listener.onNowPlaying(nowPlaying) }
        message.controller?.optJSONArray("supported_commands")?.let { commands ->
          listener.onControllerCommands((0 until commands.length()).map(commands::getString).toSet())
        }
      }
      is ServerMessage.GroupUpdate -> message.playbackState?.let(listener::onPlaybackState)
      is ServerMessage.Command -> {
        when (message.command) {
          "volume" -> message.volume?.let { volume = it.coerceIn(0, 100) }
          "mute" -> message.mute?.let { muted = it }
        }
        listener.onVolume(volume, muted)
        sendState()
      }
      is ServerMessage.Unknown -> Unit
    }
  }

  private fun startBurst(feedAll: Boolean) {
    burstRemaining = BURST_SIZE
    burstBest = null
    burstFeedsAll = feedAll
    sendTime()
  }

  private fun sendTime() {
    handler.removeCallbacks(burstTimeout)
    handler.postDelayed(burstTimeout, BURST_REPLY_TIMEOUT_MS)
    socket?.send(SendspinProtocol.time(monotonicUs()))
  }

  private fun timeSample(time: ServerMessage.Time, receivedUs: Long) {
    if (burstRemaining <= 0) return
    val t1 = time.clientTransmitted
    val offset = ((time.serverReceived - t1) + (time.serverTransmitted - receivedUs)) / 2
    val maxError = ((receivedUs - t1) - (time.serverTransmitted - time.serverReceived)) / 2
    if (burstFeedsAll) synchronized(filter) { filter.update(offset, maxError, receivedUs) }
    else if (burstBest.let { it == null || maxError < it.second }) burstBest = Triple(offset, maxError, receivedUs)
    burstRemaining--
    if (burstRemaining > 0) sendTime() else finishBurst()
  }

  private fun finishBurst() {
    handler.removeCallbacks(burstTimeout)
    burstRemaining = 0
    burstBest?.let { (offset, maxError, at) -> synchronized(filter) { filter.update(offset, maxError, at) } }
    burstBest = null
    val synchronized = synchronized(filter) { filter.isSynchronized }
    if (synchronized && !announcedAvailable) {
      announcedAvailable = true
      sendState()
      setStatus(Status.CONNECTED, null)
    }
    handler.removeCallbacks(nextBurst)
    handler.postDelayed(nextBurst, if (synchronized) BURST_INTERVAL_MS else BURST_RETRY_MS)
  }

  private fun sendState() {
    socket?.send(SendspinProtocol.state(announcedAvailable, volume, muted, LEAD_TIME_MS))
  }

  private fun dropped(code: Int?, reason: String) {
    socket = null
    handler.removeCallbacks(nextBurst)
    handler.removeCallbacks(burstTimeout)
    listener.onStreamEnd()
    if (closing) return
    if (code == UNAUTHORIZED_CLOSE_CODE) {
      setStatus(Status.UNAUTHORIZED, reason)
      return
    }
    if (attempt >= MAX_RECONNECT_ATTEMPTS) {
      setStatus(Status.CLOSED, reason)
      return
    }
    setStatus(Status.RECONNECTING, reason)
    val delay = minOf(30_000L, 1000L shl attempt)
    attempt++
    handler.postDelayed({ if (!closing) open() }, delay)
  }

  private fun setStatus(next: Status, message: String?) {
    if (status == next && message == null) return
    status = next
    listener.onStatus(next, message)
  }

  companion object {
    /** Formats in order of preference. PCM needs no decoder; FLAC comes later. */
    val SUPPORTED_FORMATS = listOf(AudioFormat("pcm", 48_000, 2, 16), AudioFormat("pcm", 44_100, 2, 16))
    /** About five seconds of 48 kHz stereo PCM, which bridges brief network stalls. */
    const val BUFFER_CAPACITY = 1_048_576
    /** How far ahead of playback the server should schedule a stream's first chunk. */
    const val LEAD_TIME_MS = 300
    private const val BURST_SIZE = 8
    private const val BURST_REPLY_TIMEOUT_MS = 1_000L
    private const val BURST_INTERVAL_MS = 10_000L
    private const val BURST_RETRY_MS = 1_000L
    private const val MAX_RECONNECT_ATTEMPTS = 8
    /** The server closes with this when the token is refused. */
    private const val UNAUTHORIZED_CLOSE_CODE = 4001
  }
}
