/** Where Spotify sends the browser back to after logging in: this server, as the web UI reaches it. */
export function callbackUrl(origin: string): string {
  return `${origin.replace(/\/+$/, '')}/spotify/callback`;
}
