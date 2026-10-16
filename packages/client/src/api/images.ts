import type { MediaImage, WithImages } from './schemas/media';

/** The sizes the image proxy accepts; anything else is refused. */
const SIZES = [80, 160, 256, 512, 1024] as const;
export type ImageSize = (typeof SIZES)[number];

/** Music Assistant 2.7 served the image proxy only on its stream server. */
const LEGACY_STREAM_PORT = '8097';

function primaryImage(item: WithImages): MediaImage | null {
  if ('metadata' in item) {
    const images = item.metadata.images;
    return images.find((candidate) => candidate.type === 'thumb') ?? images[0] ?? null;
  }
  return item.image ?? null;
}

/**
 * A resized artwork URL for an item, through the server's image proxy. The
 * proxy needs no token. From 2.10 images carry a `proxy_id`; older servers
 * take the provider and path as query parameters on the stream port.
 */
export function imageUrl(serverUrl: string, item: WithImages, size: ImageSize): string | null {
  const image = primaryImage(item);
  if (!image) return null;
  const base = serverUrl.replace(/\/+$/, '');
  if (image.proxy_id) return `${base}/imageproxy/${image.proxy_id}?size=${size}&fmt=jpeg`;
  // A public image, such as Spotify's artwork for what a source plays, is used as it is.
  if (image.remotely_accessible && /^https?:\/\//.test(image.path)) {
    return reachableImageUrl(base, image.path);
  }

  const stream = new URL(base);
  stream.protocol = 'http:';
  stream.port = LEGACY_STREAM_PORT;
  stream.pathname = '/imageproxy';
  // The legacy proxy decodes the path one more time than the query string does.
  const path = encodeURIComponent(encodeURIComponent(image.path));
  return `${stream.toString()}?provider=${encodeURIComponent(image.provider)}&size=${size}&fmt=jpeg&path=${path}`;
}

/**
 * What names an item's picture whatever size it is asked for, so an image cache can count the
 * proxy's sizes of it as one: the proxy id, or the provider and path. Null without images.
 */
export function imageKey(item: WithImages): string | null {
  const image = primaryImage(item);
  if (!image) return null;
  return image.proxy_id ?? `${image.provider}:${image.path}`;
}

/**
 * An artwork URL the server sent whole, as a player's `current_media.image_url` is, made
 * reachable from here. The server writes its own image proxy's URLs from the address it
 * knows itself by, which can be one only its network reaches, over plain HTTP; those are
 * moved onto `serverUrl`, the address this client reached it at. Other URLs, such as a
 * catalog's artwork, are kept as they are.
 */
export function reachableImageUrl(serverUrl: string, url: string): string {
  let parsed: URL;
  try {
    parsed = new URL(url, serverUrl);
  } catch {
    return url;
  }
  if (!parsed.pathname.startsWith('/imageproxy')) return url;
  return `${serverUrl.replace(/\/+$/, '')}${parsed.pathname}${parsed.search}`;
}
