/**
 * Normalizes what a person types as a server address and drops trailing
 * slashes. A bare host, typed without a scheme, is taken to be Music
 * Assistant on the local network: plain HTTP on its default port, 8095,
 * unless a port is given. An address with a scheme is used as typed, since
 * it usually points at a reverse proxy.
 */
export function normalizeServerUrl(input: string): string {
  const trimmed = input.trim().replace(/\/+$/, '');
  const hasScheme = /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed);
  const url = new URL(hasScheme ? trimmed : `http://${trimmed}`);
  if (!hasScheme && !url.port) url.port = '8095';
  return url.toString().replace(/\/+$/, '');
}

/** The WebSocket URL for a path on the server, keeping any path prefix of a reverse proxy. */
export function socketUrl(serverUrl: string, path: string): string {
  const url = new URL(serverUrl);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  url.pathname = `${url.pathname.replace(/\/+$/, '')}${path}`;
  return url.toString();
}
