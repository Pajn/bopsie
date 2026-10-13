import { useState } from 'react';
import { KeyboardAvoidingView, Platform, ScrollView } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useAccountActions, useAccounts, useActiveAccount } from '@/auth/AuthProvider';
import { signIn } from '@/auth/signIn';
import { describeError } from '@/lib/errors';
import { Button, InlineError, Screen, Stack, Text, TextField } from '@/ui/primitives';

const DEVICE_NAME = Platform.OS === 'android' ? 'Bopsie on Android' : 'Bopsie on iOS';

export default function SignInScreen() {
  const { addAccount } = useAccountActions();
  const active = useActiveAccount();
  const expired = useAccounts((state) =>
    active ? state.expiredAccountIds.includes(active.id) : false,
  );
  const [serverUrl, setServerUrl] = useState(expired && active ? active.serverUrl : '');
  const [username, setUsername] = useState(expired && active ? active.username : '');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const { account, token } = await signIn({
        serverUrl,
        username,
        password,
        deviceName: DEVICE_NAME,
      });
      await addAccount(account, token);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(false);
    }
  };

  const ready = serverUrl.trim().length > 0 && username.length > 0 && password.length > 0;

  return (
    <Screen>
      {/* Edge to edge, Android no longer resizes the window for the keyboard,
          so both platforms pad the form up out of its way. */}
      <KeyboardAvoidingView behavior="padding" style={styles.fill}>
        <ScrollView contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled">
          <Stack gap="lg">
            <Stack gap="sm">
              <Text variant="title">Bopsie</Text>
              <Text muted>
                {expired
                  ? 'The server no longer accepts this sign-in. Sign in again to continue.'
                  : 'Sign in to your Music Assistant server.'}
              </Text>
            </Stack>
            <Stack>
              <TextField
                label="Server"
                placeholder="192.168.1.10 or https://music.example.com"
                autoCapitalize="none"
                autoCorrect={false}
                keyboardType="url"
                value={serverUrl}
                onChangeText={setServerUrl}
                testID="login-server"
              />
              <TextField
                label="Username"
                autoCapitalize="none"
                autoCorrect={false}
                textContentType="username"
                value={username}
                onChangeText={setUsername}
                testID="login-username"
              />
              <TextField
                label="Password"
                secureTextEntry
                textContentType="password"
                value={password}
                onChangeText={setPassword}
                returnKeyType="go"
                onSubmitEditing={() => ready && submit()}
                testID="login-password"
              />
              <Button
                title="Sign in"
                loading={busy}
                disabled={busy || !ready}
                onPress={submit}
                testID="login-submit"
              />
            </Stack>
            <InlineError message={error} testID="login-error" />
          </Stack>
        </ScrollView>
      </KeyboardAvoidingView>
    </Screen>
  );
}

const styles = StyleSheet.create((theme) => ({
  fill: { flex: 1 },
  content: { flexGrow: 1, justifyContent: 'center', paddingVertical: theme.spacing.xxl },
}));
