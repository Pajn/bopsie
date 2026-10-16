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
import { IconButton, Tabs } from '@/ui/controls';
import { Badge, Button, Card, InlineError, Page, Stack, Text, TextField } from '@/ui/primitives';

const usersKey = (accountId: string) => ['users', accountId] as const;
const tokensKey = (accountId: string, userId: string) => ['users', accountId, userId] as const;

function when(iso: string | null | undefined): string {
  return iso ? new Date(iso).toLocaleString() : '–';
}

function day(iso: string | null | undefined): string {
  return iso ? new Date(iso).toLocaleDateString() : '–';
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

/** A round mark with the first letter of a name, in place of a picture. */
function Initial({ name }: { name: string }) {
  return (
    <View style={styles.avatar}>
      <Text style={styles.initial}>{name.slice(0, 1).toUpperCase()}</Text>
    </View>
  );
}

function UserRow({ user, isMe, last }: { user: ManagedUser; isMe: boolean; last: boolean }) {
  const { connection } = useConnection();
  const { run, error } = useChange();
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState('');
  const [displayName, setDisplayName] = useState(user.display_name ?? '');
  const admin = user.role === 'admin';
  const name = user.display_name || user.username;
  return (
    <View style={[styles.user, !last && styles.divided]}>
      <View style={styles.header}>
        <Initial name={name} />
        <View style={styles.grow}>
          <Text variant="heading" numberOfLines={1}>
            {name}
          </Text>
          <Text variant="caption" muted numberOfLines={1}>
            {user.display_name ? `${user.username} · ` : ''}since {day(user.created_at)}
          </Text>
        </View>
        <Stack horizontal gap="xs">
          {isMe ? <Badge label="you" tone="accent" /> : null}
          {admin ? <Badge label="admin" tone="accent" /> : null}
          {user.enabled ? null : <Badge label="disabled" tone="danger" />}
        </Stack>
        <IconButton
          icon={open ? 'chevron-up' : 'settings'}
          label={open ? `Close ${name}` : `Manage ${name}`}
          variant="ghost"
          active={open}
          onPress={() => setOpen(!open)}
        />
      </View>
      {open ? (
        <Stack gap="md" style={styles.manage}>
          <View style={styles.fieldRow}>
            <View style={styles.field}>
              <TextField label="Display name" value={displayName} onChangeText={setDisplayName} />
            </View>
            <Button
              title="Save"
              variant="secondary"
              disabled={displayName === (user.display_name ?? '')}
              onPress={() =>
                void run(() => updateUser(connection, { userId: user.user_id, displayName }))
              }
            />
          </View>
          <View style={styles.fieldRow}>
            <View style={styles.field}>
              <TextField
                label="New password, 8 characters or more"
                secureTextEntry
                value={password}
                onChangeText={setPassword}
              />
            </View>
            <Button
              title="Set password"
              variant="secondary"
              disabled={password.length < 8}
              onPress={() =>
                void run(async () => {
                  await updateUser(connection, { userId: user.user_id, password });
                  setPassword('');
                })
              }
            />
          </View>
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
                  icon="trash-2"
                  variant="danger"
                  onPress={() => {
                    // Deleting cannot be undone, so it asks first.
                    if (window.confirm(`Delete ${name}? They are signed out everywhere.`)) {
                      void run(() => deleteUser(connection, user.user_id));
                    }
                  }}
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
    </View>
  );
}

const ROLES = [
  { value: 'user', label: 'User' },
  { value: 'admin', label: 'Admin' },
] as const;

function NewUser() {
  const { connection } = useConnection();
  const { run, error } = useChange();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [role, setRole] = useState<'admin' | 'user'>('user');
  return (
    <Card title="Add a user">
      <View style={styles.fieldRow}>
        <View style={styles.field}>
          <TextField
            label="Username"
            autoCapitalize="none"
            value={username}
            onChangeText={setUsername}
          />
        </View>
        <View style={styles.field}>
          <TextField
            label="Password, 8 characters or more"
            secureTextEntry
            value={password}
            onChangeText={setPassword}
          />
        </View>
      </View>
      <View style={styles.fieldRow}>
        <Tabs tabs={ROLES} value={role} onChange={setRole} />
        <Text variant="caption" muted style={styles.grow}>
          {role === 'admin'
            ? 'Manages the server, its players and its users, as you do.'
            : 'Plays music and controls the players.'}
        </Text>
        <Button
          title="Add user"
          icon="user-plus"
          disabled={username.trim().length < 2 || password.length < 8}
          onPress={() =>
            void run(async () => {
              await createUser(connection, { username, password, role });
              setUsername('');
              setPassword('');
            })
          }
        />
      </View>
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
    <Page title="Users" subtitle="Who can sign in, and what they may do">
      <InlineError message={users.error ? describeError(users.error) : null} />
      {users.data?.length ? (
        <Card style={styles.list}>
          {users.data.map((user, i) => (
            <UserRow
              key={user.user_id}
              user={user}
              isMe={user.user_id === account.userId}
              last={i === users.data.length - 1}
            />
          ))}
        </Card>
      ) : null}
      <NewUser />
    </Page>
  );
}

const styles = StyleSheet.create((theme) => ({
  list: { padding: 0, gap: 0 },
  user: { padding: theme.spacing.lg, gap: theme.spacing.md },
  divided: { borderBottomWidth: 1, borderColor: theme.colors.border },
  header: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.md },
  avatar: {
    width: 40,
    height: 40,
    borderRadius: 20,
    alignItems: 'center',
    justifyContent: 'center',
    backgroundColor: theme.colors.surfaceVariant,
  },
  initial: { fontWeight: '700', color: theme.colors.accent },
  grow: { flex: 1, minWidth: 0 },
  // Under the avatar's edge, so what is managed lines up with the name.
  manage: { paddingLeft: { xs: 0, md: 40 + theme.spacing.md } },
  // Side by side where there is room, a field above the next on a phone.
  fieldRow: {
    flexDirection: { xs: 'column', md: 'row' },
    alignItems: { xs: 'stretch', md: 'flex-end' },
    gap: theme.spacing.sm,
  },
  field: { flexGrow: 1, flexBasis: { xs: 'auto', md: 0 } },
  token: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.sm,
    paddingVertical: theme.spacing.xs,
    borderBottomWidth: 1,
    borderColor: theme.colors.border,
  },
}));
