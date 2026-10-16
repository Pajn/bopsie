import {
  createUser,
  deleteUser,
  listTokens,
  listUsers,
  revokeToken,
  setUserEnabled,
  updateUser,
} from '@bopsie/client/api/endpoints/users';
import type { ManagedUser } from '@bopsie/client/api/schemas/users';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { describeError } from '@/lib/errors';
import { Badge, Button, Card, InlineError, Page, Stack, Text, TextField } from '@/ui/primitives';

const usersKey = (accountId: string) => ['users', accountId] as const;
const tokensKey = (accountId: string, userId: string) => ['users', accountId, userId] as const;

function when(iso: string | null | undefined): string {
  return iso ? new Date(iso).toLocaleString() : '–';
}

/** Runs a change, then reads users again; says what went wrong. */
function useChange() {
  const { account } = useConnection();
  const queryClient = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const run = async (change: () => Promise<unknown>) => {
    setError(null);
    try {
      await change();
      await queryClient.invalidateQueries({ queryKey: usersKey(account.id) });
    } catch (e) {
      setError(describeError(e));
    }
  };
  return { run, error };
}

function SignIns({ user }: { user: ManagedUser }) {
  const { account, connection } = useConnection();
  const { run, error } = useChange();
  const tokens = useQuery({
    queryKey: tokensKey(account.id, user.user_id),
    queryFn: () => listTokens(connection, user.user_id),
  });
  return (
    <Stack gap="xs">
      <Text variant="label" muted>
        SIGN-INS
      </Text>
      {tokens.data?.length === 0 ? <Text muted>None.</Text> : null}
      {tokens.data?.map((token) => (
        <View key={token.token_id} style={styles.token}>
          <View style={styles.grow}>
            <Text>{token.name}</Text>
            <Text variant="caption" muted>
              {token.is_long_lived ? 'Long-lived' : 'Session'} · created {when(token.created_at)} ·
              last used {when(token.last_used_at)}
            </Text>
          </View>
          <Button
            title="Revoke"
            variant="secondary"
            onPress={() => void run(() => revokeToken(connection, token.token_id))}
          />
        </View>
      ))}
      <InlineError message={error ?? (tokens.error ? describeError(tokens.error) : null)} />
    </Stack>
  );
}

function UserCard({ user, isMe }: { user: ManagedUser; isMe: boolean }) {
  const { connection } = useConnection();
  const { run, error } = useChange();
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState('');
  const [displayName, setDisplayName] = useState(user.display_name ?? '');
  const admin = user.role === 'admin';
  return (
    <Card>
      <View style={styles.header}>
        <Stack gap="xs" style={styles.grow}>
          <Text variant="heading">{user.display_name || user.username}</Text>
          <Text variant="caption" muted>
            {user.username} · since {when(user.created_at)}
          </Text>
        </Stack>
        <Stack horizontal gap="xs">
          {isMe ? <Badge label="you" tone="accent" /> : null}
          <Badge label={user.role} tone={admin ? 'accent' : 'muted'} />
          {user.enabled ? null : <Badge label="disabled" tone="danger" />}
          <Button
            title={open ? 'Close' : 'Manage'}
            variant="secondary"
            onPress={() => setOpen(!open)}
          />
        </Stack>
      </View>
      {open ? (
        <Stack gap="md">
          <Stack horizontal gap="xs">
            <TextField
              label="Display name"
              value={displayName}
              onChangeText={setDisplayName}
              style={styles.input}
            />
            <Button
              title="Save"
              onPress={() =>
                void run(() => updateUser(connection, { userId: user.user_id, displayName }))
              }
            />
          </Stack>
          <Stack horizontal gap="xs">
            <TextField
              label="New password"
              secureTextEntry
              value={password}
              onChangeText={setPassword}
              style={styles.input}
            />
            <Button
              title="Set password"
              disabled={password.length < 8}
              onPress={() =>
                void run(async () => {
                  await updateUser(connection, { userId: user.user_id, password });
                  setPassword('');
                })
              }
            />
          </Stack>
          <Stack horizontal gap="xs">
            <Button
              title={admin ? 'Make a user' : 'Make an admin'}
              variant="secondary"
              onPress={() =>
                void run(() =>
                  updateUser(connection, { userId: user.user_id, role: admin ? 'user' : 'admin' }),
                )
              }
            />
            {isMe ? null : (
              <>
                <Button
                  title={user.enabled ? 'Disable' : 'Enable'}
                  variant="secondary"
                  onPress={() =>
                    void run(() => setUserEnabled(connection, user.user_id, !user.enabled))
                  }
                />
                <Button
                  title="Delete"
                  variant="secondary"
                  onPress={() => void run(() => deleteUser(connection, user.user_id))}
                />
              </>
            )}
          </Stack>
          <Text variant="caption" muted>
            Disabling or deleting a user, or making an admin a user, signs them out everywhere.
          </Text>
          <InlineError message={error} />
          <SignIns user={user} />
        </Stack>
      ) : null}
    </Card>
  );
}

function NewUser() {
  const { connection } = useConnection();
  const { run, error } = useChange();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [role, setRole] = useState<'admin' | 'user'>('user');
  return (
    <Card title="New user">
      <Stack horizontal gap="sm">
        <TextField
          label="Username"
          autoCapitalize="none"
          value={username}
          onChangeText={setUsername}
          style={styles.input}
        />
        <TextField
          label="Password, 8 characters or more"
          secureTextEntry
          value={password}
          onChangeText={setPassword}
          style={styles.input}
        />
      </Stack>
      <Stack horizontal gap="xs">
        <Button
          title="User"
          variant={role === 'user' ? 'primary' : 'secondary'}
          onPress={() => setRole('user')}
        />
        <Button
          title="Admin"
          variant={role === 'admin' ? 'primary' : 'secondary'}
          onPress={() => setRole('admin')}
        />
        <Button
          title="Create"
          disabled={username.trim().length < 2 || password.length < 8}
          onPress={() =>
            void run(async () => {
              await createUser(connection, { username, password, role });
              setUsername('');
              setPassword('');
            })
          }
        />
      </Stack>
      <InlineError message={error} />
    </Card>
  );
}

export default function UsersScreen() {
  const { account, connection } = useConnection();
  const users = useQuery({
    queryKey: usersKey(account.id),
    queryFn: () => listUsers(connection),
  });
  return (
    <Page title={'Users'}>
      <InlineError message={users.error ? describeError(users.error) : null} />
      {users.data?.map((user) => (
        <UserCard key={user.user_id} user={user} isMe={user.user_id === account.userId} />
      ))}
      <NewUser />
    </Page>
  );
}

const styles = StyleSheet.create((theme) => ({
  header: {
    flexDirection: 'row',
    alignItems: 'center',
    flexWrap: 'wrap',
    gap: theme.spacing.sm,
  },
  grow: { flex: 1, minWidth: 0 },
  input: { minWidth: 200 },
  token: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.sm,
    paddingVertical: theme.spacing.xs,
    borderBottomWidth: 1,
    borderColor: theme.colors.border,
  },
}));
