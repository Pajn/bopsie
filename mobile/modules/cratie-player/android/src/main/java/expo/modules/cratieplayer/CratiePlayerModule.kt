package expo.modules.cratieplayer

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.IBinder
import androidx.media3.common.util.UnstableApi
import expo.modules.kotlin.Promise
import expo.modules.kotlin.functions.Queues
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import java.util.UUID

@UnstableApi
class CratiePlayerModule : Module() {
  private var service: PlayerService? = null
  private var bound = false
  private var bindingContext: Context? = null
  private val waiting = mutableListOf<Promise>()
  private val connection = object : ServiceConnection {
    override fun onServiceConnected(name: ComponentName, binder: IBinder) {
      service = (binder as PlayerService.LocalBinder).service.also {
        it.listener = { json -> sendEvent("player", mapOf("json" to json)) }
      }
      waiting.toList().also { waiting.clear() }.forEach { it.resolve(null) }
    }
    override fun onServiceDisconnected(name: ComponentName) { service = null }
  }
  private fun context() = requireNotNull(appContext.reactContext)
  private fun player() = requireNotNull(service) { "Set up the player first" }

  override fun definition() = ModuleDefinition {
    Name("CratiePlayer")
    Events("player")

    /** This install's Sendspin client id, created on first use. It is the player's id in Music Assistant. */
    Function("clientId") {
      val preferences = context().getSharedPreferences("cratie-player", Context.MODE_PRIVATE)
      preferences.getString("clientId", null) ?: UUID.randomUUID().toString().also {
        preferences.edit().putString("clientId", it).apply()
      }
    }

    AsyncFunction("setup") { promise: Promise ->
      if (service != null) promise.resolve(null)
      else {
        waiting.add(promise)
        if (!bound) {
          val context = context().applicationContext
          bindingContext = context
          bound = context.bindService(Intent(context, PlayerService::class.java).setAction(PlayerService.LOCAL_BIND), connection, Context.BIND_AUTO_CREATE)
          if (!bound) waiting.toList().also { waiting.clear() }.forEach {
            it.reject("PLAYER_BIND", "Could not connect to the player service", null)
          }
        }
      }
    }.runOnQueue(Queues.MAIN)

    AsyncFunction("connect") { json: String -> player().connect(json) }.runOnQueue(Queues.MAIN)
    AsyncFunction("disconnect") { service?.disconnect() }.runOnQueue(Queues.MAIN)
    AsyncFunction("snapshot") { player().snapshot() }.runOnQueue(Queues.MAIN)

    OnDestroy {
      service?.listener = null
      if (bound) bindingContext?.unbindService(connection)
      bindingContext = null
      service = null
      bound = false
    }
  }
}
