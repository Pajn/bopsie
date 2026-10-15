import { router } from 'expo-router';
import { Alert } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useAccountActions } from '@/auth/AuthProvider';
import { useConnection } from '@/connection/ConnectionProvider';
import type { PlayerStatus } from '@/player/Player';
import { Button, Screen, Stack, Text } from '@/ui/primitives';

const PLAYER_STATUS: Record<PlayerStatus, string> = {
  unsupported: 'Playing on this device is Android-only for now.',
  closed: 'Not connected.',
  connecting: 'Connecting…',
  connected: 'Ready. Music Assistant lists this phone as a player.',
  reconnecting: 'Connection lost. Reconnecting…',
  unauthorized: 'The server refused this sign-in.',
};

export function AccountScreen() {
  const { account, connection, playerStatus } = useConnection();
  const { removeAccount } = useAccountActions();
  const server = connection.serverInfo;

  return (
    <Screen>
      <Stack gap="lg" style={styles.content}>
        <Text variant="title">Account</Text>
        <Stack gap="sm">
          <Text variant="label" muted>
            Signed in as
          </Text>
          <Text testID="account-username">{account.username}</Text>
          <Text muted>
            {account.serverUrl}
            {server ? ` · Music Assistant ${server.server_version}` : ''}
          </Text>
        </Stack>
        <Stack gap="sm">
          <Text variant="label" muted>
            This phone as a player
          </Text>
          <Text testID="player-status">{PLAYER_STATUS[playerStatus]}</Text>
        </Stack>
        {playerStatus !== 'unsupported' && (
          <Stack gap="sm">
            <Text variant="label" muted>
              Speakers
            </Text>
            <Text muted>Put a group's speakers in step, using this phone's microphone.</Text>
            <Button
              title="Calibrate speaker sync"
              variant="secondary"
              onPress={() => router.push('/calibrate')}
              testID="account-calibrate"
            />
          </Stack>
        )}
        <Button
          title="Sign out"
          variant="secondary"
          onPress={() =>
            Alert.alert('Sign out?', `${account.username} on ${account.serverUrl}`, [
              { text: 'Cancel', style: 'cancel' },
              {
                text: 'Sign out',
                style: 'destructive',
                onPress: async () => {
                  // Revokes this device's token on the server; signing out
                  // locally goes ahead even when the server cannot be reached.
                  await connection
                    .command('auth/logout', {}, { timeoutMs: 3000 })
                    .catch(() => undefined);
                  await removeAccount(account.id);
                },
              },
            ])
          }
          testID="account-sign-out"
        />
      </Stack>
    </Screen>
  );
}

const styles = StyleSheet.create((theme) => ({
  content: { paddingTop: theme.spacing.md },
}));
