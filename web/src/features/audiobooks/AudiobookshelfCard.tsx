import {
  signInToAudiobookshelf,
  signOutOfAudiobookshelf,
} from '@bopsie/client/api/endpoints/audiobookshelf';
import { useState } from 'react';
import { useConnection } from '@/connection/ConnectionProvider';
import { useAudiobookshelf } from '@/data/admin';
import { describeError } from '@/lib/errors';
import { Button, Card, Facts, InlineError, Stack, Text, TextField } from '@/ui/primitives';

/**
 * The Audiobookshelf server audiobooks come from. Signing in hands the password to the server
 * once; it keeps the tokens Audiobookshelf gives back, never the password.
 */
export function AudiobookshelfCard() {
  const { connection } = useConnection();
  const status = useAudiobookshelf();
  // What was typed; until then, the address and user it was last signed in with.
  const [url, setUrl] = useState<string | null>(null);
  const [username, setUsername] = useState<string | null>(null);
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const data = status.data;
  const run = (work: () => Promise<unknown>) => {
    setError(null);
    setBusy(true);
    work()
      .then(() => status.refetch())
      .catch((e: unknown) => setError(describeError(e)))
      .finally(() => setBusy(false));
  };

  if (!data) {
    return (
      <Card title="Audiobookshelf">
        <InlineError message={status.error ? describeError(status.error) : null} />
      </Card>
    );
  }
  if (data.signed_in) {
    return (
      <Card title="Audiobookshelf">
        <Stack gap="md">
          <Facts
            rows={[
              ['Server', data.url ?? '–'],
              ['Signed in as', data.user ?? '–'],
            ]}
          />
          <InlineError message={error} />
          <Stack horizontal gap="sm">
            <Button
              title="Sign out"
              variant="secondary"
              loading={busy}
              onPress={() => run(() => signOutOfAudiobookshelf(connection))}
            />
          </Stack>
        </Stack>
      </Card>
    );
  }
  const shownUrl = url ?? data.url ?? '';
  const shownUser = username ?? data.user ?? '';
  return (
    <Card title="Audiobookshelf">
      <Stack gap="md">
        <Text muted>
          {data.user
            ? `Signed out; ${data.user} was signed in last. Sign in again to play its audiobooks.`
            : 'Sign in to an Audiobookshelf server to play its audiobooks. The password is used once and not kept.'}
        </Text>
        <TextField
          label="Address"
          value={shownUrl}
          onChangeText={setUrl}
          autoCapitalize="none"
          placeholder="http://audiobooks.local:13378"
        />
        <TextField
          label="Username"
          value={shownUser}
          onChangeText={setUsername}
          autoCapitalize="none"
        />
        <TextField
          label="Password"
          value={password}
          onChangeText={setPassword}
          secureTextEntry
          autoCapitalize="none"
        />
        <InlineError message={error} />
        <Stack horizontal gap="sm">
          <Button
            title="Sign in"
            loading={busy}
            disabled={shownUrl.trim() === '' || shownUser.trim() === '' || password === ''}
            onPress={() =>
              run(async () => {
                await signInToAudiobookshelf(
                  connection,
                  shownUrl.trim(),
                  shownUser.trim(),
                  password,
                );
                setPassword('');
              })
            }
          />
        </Stack>
      </Stack>
    </Card>
  );
}
