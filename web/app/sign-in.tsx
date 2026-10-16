import { signIn } from '@bopsie/client/auth/signIn';
import { useState } from 'react';
import { Image, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useAccountActions, useAccounts, useActiveAccount } from '@/auth/AuthProvider';
import { describeError } from '@/lib/errors';
import { Button, Card, InlineError, Stack, Text, TextField } from '@/ui/primitives';

const ICON = require('../assets/mark.png');

/** The page is served by the server it manages, so that is the server to sign in to. */
function serverUrl(): string {
  return typeof window === 'undefined' ? '' : window.location.origin;
}

export default function SignInScreen() {
  const { addAccount } = useAccountActions();
  const active = useActiveAccount();
  const expired = useAccounts((state) =>
    active ? state.expiredAccountIds.includes(active.id) : false,
  );
  const [username, setUsername] = useState(expired && active ? active.username : '');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const { account, token } = await signIn({
        serverUrl: serverUrl(),
        username,
        password,
        deviceName: 'Bopsie admin',
      });
      await addAccount(account, token);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(false);
    }
  };

  const ready = username.length > 0 && password.length > 0 && !busy;

  return (
    <View style={styles.page}>
      <Card style={styles.card}>
        <Stack gap="lg">
          <Stack gap="xs">
            <Image source={ICON} style={styles.icon} accessibilityIgnoresInvertColors />
            <Text variant="title">Bopsie</Text>
            <Text muted>
              {expired
                ? 'The server no longer accepts this sign-in. Sign in again to continue.'
                : 'Sign in with an admin account to manage this server.'}
            </Text>
          </Stack>
          <Stack>
            <TextField
              label="Username"
              autoFocus={!username}
              autoCapitalize="none"
              autoCorrect={false}
              autoComplete="username"
              value={username}
              onChangeText={setUsername}
            />
            <TextField
              label="Password"
              autoFocus={Boolean(username)}
              secureTextEntry
              autoComplete="current-password"
              value={password}
              onChangeText={setPassword}
              onSubmitEditing={() => ready && void submit()}
            />
          </Stack>
          <InlineError message={error} />
          <Button title="Sign in" loading={busy} disabled={!ready} onPress={() => void submit()} />
        </Stack>
      </Card>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  page: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    padding: theme.spacing.lg,
    backgroundColor: theme.colors.background,
  },
  card: { width: '100%', maxWidth: 380 },
  icon: { width: 56, height: 56, borderRadius: 14, marginBottom: theme.spacing.sm },
}));
