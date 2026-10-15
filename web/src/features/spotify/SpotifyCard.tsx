import {
  cancelSpotifyPairing,
  setSpotifyClientId,
  signOutOfSpotify,
  startSpotifyLogin,
  startSpotifyPairing,
} from '@bopsie/client/api/endpoints/spotify';
import { useState } from 'react';
import { useConnection } from '@/connection/ConnectionProvider';
import { useSpotify } from '@/data/admin';
import { describeError } from '@/lib/errors';
import { Button, Card, Facts, InlineError, Stack, Text, TextField } from '@/ui/primitives';
import { callbackUrl } from './model';

/**
 * The server's Spotify account: its developer app and Web API login, for search and the saved
 * library, and its playback login, made by picking a device in the Spotify app. `outcome` is
 * what a login that came back to this page said.
 */
export function SpotifyCard({ outcome }: { outcome: string | null }) {
  const { connection } = useConnection();
  const spotify = useSpotify();
  const [clientId, setClientId] = useState('');
  const [error, setError] = useState<string | null>(null);
  const status = spotify.data;
  const run = (work: () => Promise<unknown>) => {
    setError(null);
    work()
      .then(() => spotify.refetch())
      .catch((e: unknown) => setError(describeError(e)));
  };
  const redirect = callbackUrl(window.location.origin);

  if (!status) {
    return (
      <Card title="Spotify">
        <InlineError message={spotify.error ? describeError(spotify.error) : null} />
      </Card>
    );
  }
  if (!status.enabled) {
    return (
      <Card title="Spotify">
        <Text muted>Turned off in the config file.</Text>
      </Card>
    );
  }
  return (
    <Card title="Spotify">
      <Stack gap="md">
        <Facts
          rows={[
            ['Search and saved library', status.web_user ?? 'not logged in'],
            ['Playback and playlists', status.playback_user ?? 'not signed in'],
            ['Developer app', status.client_id ?? 'none'],
          ]}
        />
        <InlineError message={error ?? outcome ?? status.error} />

        <Text variant="label" muted>
          Search and saved library
        </Text>
        {status.client_id ? (
          <Stack horizontal gap="sm">
            <Button
              title={status.web_user ? 'Log in again' : 'Log in with Spotify'}
              onPress={() =>
                run(async () => {
                  const { url } = await startSpotifyLogin(connection, redirect);
                  window.location.assign(url);
                })
              }
            />
            {status.web_user ? (
              <Button
                title="Log out"
                variant="secondary"
                onPress={() => run(() => signOutOfSpotify(connection, 'web'))}
              />
            ) : null}
            <Button
              title="Forget the app"
              variant="secondary"
              onPress={() => run(() => setSpotifyClientId(connection, null))}
            />
          </Stack>
        ) : (
          <Stack gap="sm">
            <Text muted>
              {`Create an app at developer.spotify.com with ${redirect} as a redirect URI, then give its client ID here.`}
            </Text>
            <Stack horizontal gap="sm">
              <TextField
                label="Client ID"
                value={clientId}
                onChangeText={setClientId}
                autoCapitalize="none"
              />
              <Button
                title="Use this app"
                disabled={clientId.trim() === ''}
                onPress={() => run(() => setSpotifyClientId(connection, clientId.trim()))}
              />
            </Stack>
          </Stack>
        )}

        <Text variant="label" muted>
          Playback and playlists
        </Text>
        {status.pairing ? (
          <Stack horizontal gap="sm">
            <Text>{`In the Spotify app, pick “${status.sign_in_name}” from the devices.`}</Text>
            <Button
              title="Cancel"
              variant="secondary"
              onPress={() => run(() => cancelSpotifyPairing(connection))}
            />
          </Stack>
        ) : (
          <Stack horizontal gap="sm">
            <Button
              title={status.playback_user ? 'Sign in again' : 'Sign in from the Spotify app'}
              onPress={() => run(() => startSpotifyPairing(connection))}
            />
            {status.playback_user ? (
              <Button
                title="Sign out"
                variant="secondary"
                onPress={() => run(() => signOutOfSpotify(connection, 'playback'))}
              />
            ) : null}
          </Stack>
        )}
      </Stack>
    </Card>
  );
}
