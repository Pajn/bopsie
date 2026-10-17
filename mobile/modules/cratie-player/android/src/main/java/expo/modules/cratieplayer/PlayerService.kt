package expo.modules.cratieplayer

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Binder
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import androidx.media3.common.DeviceInfo
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.DefaultMediaNotificationProvider
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import org.json.JSONObject

/**
 * Owns the phone as a player: the Sendspin connection, the audio
 * output and the media session that gives it a notification, lock-screen
 * controls and media buttons. Those controls go to the server as Sendspin
 * controller commands, so they work without the JavaScript runtime.
 *
 * While the phone plays nothing itself and the app controls another player,
 * the session shows that player instead, as the app hands it over, with its
 * volume on the volume keys; its controls then go back to the app to send.
 */
@UnstableApi
class PlayerService : MediaSessionService() {
  inner class LocalBinder : Binder() { val service = this@PlayerService }

  private val binder = LocalBinder()
  private val main = Handler(Looper.getMainLooper())
  private lateinit var player: StreamPlayer
  private lateinit var session: MediaSession
  private lateinit var audioManager: AudioManager
  private var client: SendspinClient? = null
  /** The server origin the app connected to, for rewriting artwork URLs. */
  private var serverUrl = ""
  private var output: AudioOutput? = null
  /** Identifies the current connection, so callbacks from a replaced one are ignored. */
  @Volatile private var connection = 0
  private var focusRequest: AudioFocusRequest? = null
  private var wakeLock: PowerManager.WakeLock? = null
  private var wifiLock: WifiManager.WifiLock? = null
  private var noisyRegistered = false

  /** Receives JSON events for the JavaScript side. */
  var listener: ((String) -> Unit)? = null
  var status = SendspinClient.Status.CLOSED
    private set

  private val noisy = object : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
      if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) client?.command("pause")
    }
  }

  override fun onCreate() {
    super.onCreate()
    audioManager = getSystemService(AudioManager::class.java)
    if (Build.VERSION.SDK_INT >= 26) getSystemService(NotificationManager::class.java).createNotificationChannel(
      NotificationChannel(CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW))
    setMediaNotificationProvider(DefaultMediaNotificationProvider.Builder(this).setChannelId(CHANNEL).setNotificationId(NOTIFICATION_ID).build())
    player = StreamPlayer()
    val builder = MediaSession.Builder(this, player)
    packageManager.getLaunchIntentForPackage(packageName)?.let {
      it.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
      builder.setSessionActivity(PendingIntent.getActivity(this, 0, it,
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
    }
    session = builder.build()
    // The app binds directly rather than through a MediaController, so
    // register now for Media3 to manage the notification and foreground state.
    addSession(session)
  }

  override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession = session

  override fun onBind(intent: Intent?): IBinder? =
    if (intent?.action == LOCAL_BIND) binder else super.onBind(intent)

  override fun onTaskRemoved(rootIntent: Intent?) {
    // Another player is controlled through the app, which is going away.
    player.showRemote(null)
    if (!player.playWhenReady) {
      disconnect()
      stopSelf()
    }
  }

  override fun onDestroy() {
    disconnect()
    session.release()
    player.release()
    super.onDestroy()
  }

  /** Connects the player; a new configuration replaces the current connection. */
  fun connect(json: String) {
    val config = JSONObject(json)
    disconnect()
    serverUrl = config.getString("serverUrl")
    val output = AudioOutput { serverTime -> client?.clientTimeOf(serverTime) }
    this.output = output
    client = SendspinClient(
      SendspinClient.Config(
        url = config.getString("url"),
        token = config.getString("token"),
        clientId = config.getString("clientId"),
        name = config.getString("name"),
        softwareVersion = config.getString("softwareVersion"),
      ),
      Events(++connection),
    ).also { it.connect() }
  }

  fun disconnect() {
    connection++
    client?.release()
    client = null
    output?.release()
    output = null
    streamEnded()
    main.post { player.reset() }
  }

  /** Shows another player in the session, as the app follows it, or stops showing one. */
  fun showRemote(json: String?) {
    player.showRemote(json?.let(RemoteNowPlaying::parse))
  }

  /** A command from the session for the app to send to the player it controls. */
  private fun remoteCommand(command: String, build: JSONObject.() -> Unit = {}) {
    listener?.invoke(JSONObject().put("type", "command").put("command", command).apply(build).toString())
  }

  /** When [clientTimeUs], on the monotonic clock, falls on the server's clock: null unless connected and in step. */
  fun serverTimeOf(clientTimeUs: Long): Long? = client?.serverTimeOf(clientTimeUs)

  fun snapshot(): String = JSONObject().put("type", "status").put("status", status.name.lowercase()).toString()

  /** Sendspin callbacks, which arrive on the client's threads. */
  private inner class Events(private val id: Int) : SendspinClient.Listener {
    private val current get() = id == connection

    override fun onStatus(status: SendspinClient.Status, message: String?) {
      main.post {
        if (!current) return@post
        this@PlayerService.status = status
        val event = JSONObject().put("type", "status").put("status", status.name.lowercase())
        if (message != null) event.put("message", message)
        listener?.invoke(event.toString())
      }
    }

    override fun onStreamStart(format: AudioFormat) {
      if (!current) return
      output?.start(format)
      main.post { if (current) streamStarted() }
    }

    override fun onStreamClear() { if (current) output?.clear() }

    override fun onStreamEnd() {
      if (!current) return
      output?.stop()
      main.post { if (current) streamEnded() }
    }

    override fun onAudio(chunk: AudioChunk) { if (current) output?.enqueue(chunk) }

    override fun onNowPlaying(nowPlaying: NowPlaying) {
      main.post { if (current) player.update(nowPlaying = nowPlaying) }
    }

    override fun onPlaybackState(state: String) {
      main.post { if (current) player.update(playing = state == "playing") }
    }

    override fun onVolume(volume: Int, muted: Boolean) { if (current) output?.setVolume(volume, muted) }

    override fun onControllerCommands(commands: Set<String>) {
      main.post { if (current) player.update(commands = commands) }
    }
  }

  private fun streamStarted() {
    requestFocus()
    if (!noisyRegistered) {
      registerReceiver(noisy, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY))
      noisyRegistered = true
    }
    if (wakeLock == null) wakeLock = getSystemService(PowerManager::class.java)
      .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "cratie:stream").apply { acquire() }
    if (wifiLock == null) wifiLock = applicationContext.getSystemService(WifiManager::class.java)
      .createWifiLock(if (Build.VERSION.SDK_INT >= 29) WifiManager.WIFI_MODE_FULL_LOW_LATENCY else @Suppress("DEPRECATION") WifiManager.WIFI_MODE_FULL_HIGH_PERF, "cratie:stream")
      .apply { setReferenceCounted(false); acquire() }
    player.update(streaming = true)
  }

  private fun streamEnded() {
    focusRequest?.let(audioManager::abandonAudioFocusRequest)
    focusRequest = null
    if (noisyRegistered) {
      unregisterReceiver(noisy)
      noisyRegistered = false
    }
    wakeLock?.let { if (it.isHeld) it.release() }
    wakeLock = null
    wifiLock?.let { if (it.isHeld) it.release() }
    wifiLock = null
    if (::player.isInitialized) player.update(streaming = false)
  }

  private fun requestFocus() {
    if (focusRequest != null || Build.VERSION.SDK_INT < 26) return
    focusRequest = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
      .setAudioAttributes(AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_MEDIA)
        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
        .build())
      .setOnAudioFocusChangeListener({ change ->
        // Another app took the output, such as a call or a navigation prompt.
        // Pausing on the server keeps every client's view of the queue true.
        if (change == AudioManager.AUDIOFOCUS_LOSS || change == AudioManager.AUDIOFOCUS_LOSS_TRANSIENT) {
          client?.command("pause")
        }
      }, main)
      .build()
      .also(audioManager::requestAudioFocus)
  }

  /**
   * The stream as Media3 sees it: one item whose metadata and position come
   * from the server, and commands that go back to it. Or, while the phone
   * streams nothing, the other player the app controls.
   */
  private inner class StreamPlayer : SimpleBasePlayer(Looper.getMainLooper()) {
    private var nowPlaying = NowPlaying()
    private var playing = false
    private var streaming = false
    private var commands = emptySet<String>()
    private var remote: RemoteNowPlaying? = null
    /** The other player, unless the phone is playing itself. */
    private val remoting get() = remote.takeIf { !streaming }

    fun showRemote(remote: RemoteNowPlaying?) {
      this.remote = remote
      invalidateState()
    }

    fun update(nowPlaying: NowPlaying? = null, playing: Boolean? = null, streaming: Boolean? = null, commands: Set<String>? = null) {
      nowPlaying?.let { this.nowPlaying = it }
      playing?.let { this.playing = it }
      streaming?.let { this.streaming = it }
      commands?.let { this.commands = it }
      invalidateState()
    }

    fun reset() {
      nowPlaying = NowPlaying()
      playing = false
      streaming = false
      commands = emptySet()
      invalidateState()
    }

    override fun getState(): State {
      remoting?.let { return remoteState(it) }
      val available = Player.Commands.Builder()
        .addAll(Player.COMMAND_PLAY_PAUSE, Player.COMMAND_STOP, Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
          Player.COMMAND_GET_METADATA, Player.COMMAND_GET_TIMELINE)
      if ("next" in commands) available.add(Player.COMMAND_SEEK_TO_NEXT)
      if ("previous" in commands) available.addAll(Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM)
      if ("seek" in commands) available.add(Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM)

      val builder = State.Builder()
        .setAvailableCommands(available.build())
        .setPlayWhenReady(playing, Player.PLAY_WHEN_READY_CHANGE_REASON_REMOTE)
      if (nowPlaying.title == null && !streaming) {
        return builder.setPlaybackState(Player.STATE_IDLE).build()
      }
      val metadata = MediaMetadata.Builder()
        .setTitle(nowPlaying.title)
        .setArtist(nowPlaying.artist)
        .setAlbumTitle(nowPlaying.album)
        .setArtworkUri(ArtworkUrl.reachable(nowPlaying.artworkUrl, serverUrl)?.let(Uri::parse))
        .build()
      val item = MediaItemData.Builder("current")
        .setMediaItem(MediaItem.Builder().setMediaId("current").setMediaMetadata(metadata).build())
        .setMediaMetadata(metadata)
        .setDurationUs(nowPlaying.durationMs?.let { it * 1000 } ?: androidx.media3.common.C.TIME_UNSET)
        .setIsSeekable("seek" in commands)
        .build()
      val position = client?.serverNowUs()?.let(nowPlaying::positionMs) ?: nowPlaying.progressMs ?: 0
      return builder
        .setPlaylist(listOf(item))
        .setCurrentMediaItemIndex(0)
        .setPlaybackState(if (streaming || playing) Player.STATE_READY else Player.STATE_IDLE)
        .setContentPositionMs(PositionSupplier.getExtrapolating(position, if (playing) nowPlaying.speed / 1000f else 0f))
        .build()
    }

    /** The other player: one item, seekable when its length is known, its volume remote. */
    private fun remoteState(remote: RemoteNowPlaying): State {
      val available = Player.Commands.Builder()
        .addAll(Player.COMMAND_PLAY_PAUSE, Player.COMMAND_STOP, Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
          Player.COMMAND_GET_METADATA, Player.COMMAND_GET_TIMELINE, Player.COMMAND_SEEK_TO_NEXT,
          Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM)
      if (remote.durationMs != null) available.add(Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM)
      val builder = State.Builder()
      remote.deviceVolume?.let { volume ->
        available.addAll(Player.COMMAND_GET_DEVICE_VOLUME, Player.COMMAND_SET_DEVICE_VOLUME,
          Player.COMMAND_SET_DEVICE_VOLUME_WITH_FLAGS, Player.COMMAND_ADJUST_DEVICE_VOLUME,
          Player.COMMAND_ADJUST_DEVICE_VOLUME_WITH_FLAGS)
        // Remote playback with its own volume is what sends the volume keys here.
        builder
          .setDeviceInfo(DeviceInfo.Builder(DeviceInfo.PLAYBACK_TYPE_REMOTE).setMaxVolume(RemoteNowPlaying.STEPS).build())
          .setDeviceVolume(volume)
      }
      val metadata = MediaMetadata.Builder()
        .setTitle(remote.title)
        .setArtist(remote.artist)
        .setAlbumTitle(remote.album)
        .setArtworkUri(remote.artworkUrl?.let(Uri::parse))
        .build()
      val item = MediaItemData.Builder("remote")
        .setMediaItem(MediaItem.Builder().setMediaId("remote").setMediaMetadata(metadata).build())
        .setMediaMetadata(metadata)
        .setDurationUs(remote.durationMs?.let { it * 1000 } ?: androidx.media3.common.C.TIME_UNSET)
        .setIsSeekable(remote.durationMs != null)
        .build()
      return builder
        .setAvailableCommands(available.build())
        .setPlayWhenReady(remote.playing, Player.PLAY_WHEN_READY_CHANGE_REASON_REMOTE)
        .setPlaylist(listOf(item))
        .setCurrentMediaItemIndex(0)
        .setPlaybackState(Player.STATE_READY)
        .setContentPositionMs(PositionSupplier.getExtrapolating(
          remote.positionMs(System.currentTimeMillis()), if (remote.playing) 1f else 0f))
        .build()
    }

    /** Sends a transport command where the session's player is: the server, or the app. */
    private fun send(command: String, positionMs: Long? = null) {
      if (remoting != null) remoteCommand(command) { positionMs?.let { put("positionMs", it) } }
      else if (positionMs != null) client?.command(command, positionMs)
      else client?.command(command)
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
      send(if (playWhenReady) "play" else "pause")
      return Futures.immediateVoidFuture()
    }

    override fun handleStop(): ListenableFuture<*> {
      send("stop")
      return Futures.immediateVoidFuture()
    }

    override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> {
      when (seekCommand) {
        Player.COMMAND_SEEK_TO_NEXT, Player.COMMAND_SEEK_TO_NEXT_MEDIA_ITEM -> send("next")
        Player.COMMAND_SEEK_TO_PREVIOUS, Player.COMMAND_SEEK_TO_PREVIOUS_MEDIA_ITEM -> send("previous")
        else -> send("seek", positionMs)
      }
      return Futures.immediateVoidFuture()
    }

    override fun handleSetDeviceVolume(deviceVolume: Int, flags: Int): ListenableFuture<*> {
      setRemoteVolume(RemoteNowPlaying.volumeOf(deviceVolume))
      return Futures.immediateVoidFuture()
    }

    override fun handleIncreaseDeviceVolume(flags: Int): ListenableFuture<*> {
      remoting?.deviceVolume?.let { setRemoteVolume(RemoteNowPlaying.volumeOf(it + 1)) }
      return Futures.immediateVoidFuture()
    }

    override fun handleDecreaseDeviceVolume(flags: Int): ListenableFuture<*> {
      remoting?.deviceVolume?.let { setRemoteVolume(RemoteNowPlaying.volumeOf(it - 1)) }
      return Futures.immediateVoidFuture()
    }

    // Muting is not offered for another player; the volume keys only step it.
    override fun handleSetDeviceMuted(muted: Boolean, flags: Int): ListenableFuture<*> =
      Futures.immediateVoidFuture()

    /**
     * Asks the app to set the other player's volume, and shows it at once, so presses in quick
     * succession count from each other rather than from what the server last said.
     */
    private fun setRemoteVolume(level: Int) {
      val remote = remoting ?: return
      this.remote = remote.copy(volume = level)
      invalidateState()
      remoteCommand("volume") { put("level", level) }
    }
  }

  companion object {
    const val LOCAL_BIND = "expo.modules.cratieplayer.LOCAL_BIND"
    private const val CHANNEL = "playback"
    private const val NOTIFICATION_ID = 1
  }
}
