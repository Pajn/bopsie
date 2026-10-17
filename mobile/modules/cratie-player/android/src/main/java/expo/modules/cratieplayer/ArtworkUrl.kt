package expo.modules.cratieplayer

import java.net.URI

/**
 * Sendspin metadata builds artwork URLs from the address the server
 * believes it has, which inside a container or behind a reverse proxy is not
 * one the phone can reach. Image-proxy URLs are moved onto the origin the
 * app connected to, keeping any path prefix the proxy adds. Other URLs, such
 * as a streaming service's own artwork, are left as they are.
 */
object ArtworkUrl {
  fun reachable(artworkUrl: String?, serverUrl: String): String? {
    if (artworkUrl == null) return null
    val artwork = try { URI(artworkUrl) } catch (_: Exception) { return artworkUrl }
    val path = artwork.rawPath ?: return artworkUrl
    if (!path.startsWith("/imageproxy")) return artworkUrl
    val query = artwork.rawQuery?.let { "?$it" } ?: ""
    return "${serverUrl.trimEnd('/')}$path$query"
  }
}
