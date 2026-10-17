import {
  createBackup,
  deleteBackup,
  downloadBackup,
  getBackups,
  restoreBackupFile,
  restoreStoredBackup,
  setBackupSchedule,
} from '@bopsie/client/api/endpoints/backups';
import type { Backup, BackupsStatus } from '@bopsie/client/api/schemas/backups';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { POLL_MS } from '@/data/admin';
import { describeError } from '@/lib/errors';
import {
  Badge,
  Button,
  Card,
  Choice,
  Facts,
  InlineError,
  Stack,
  Text,
  TextField,
} from '@/ui/primitives';
import { formatBytes, scheduleLabel, toLocal, toUtc, WEEKDAYS } from './model';

const at = (unixSeconds: number) => new Date(unixSeconds * 1000).toLocaleString();

/** Minutes UTC is ahead of local time, in the browser. */
const offset = () => new Date().getTimezoneOffset();

/** Saves `blob` as a download named `name`. */
function save(blob: Blob, name: string) {
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

const RESTORE_WARNING =
  'Restoring replaces the whole server: users, settings, edits, history, logins and pairings. ' +
  'What it replaces is kept in before-restore-… in the data directory. The server restarts.';

/**
 * The server's backups: made now and downloaded, kept on the server, made on a schedule, and
 * restored, from one kept there or from a file.
 */
export function BackupsCard() {
  const { account, connection, token } = useConnection();
  const status = useQuery({
    queryKey: queryKeys.admin(account.id, 'backups'),
    queryFn: () => getBackups(connection),
    refetchInterval: POLL_MS,
  });
  const [passphrase, setPassphrase] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const data = status.data;

  const run = (what: string, work: () => Promise<unknown>) => {
    setError(null);
    setNotice(null);
    setBusy(what);
    work()
      .then(() => status.refetch())
      .catch((e: unknown) => setError(describeError(e)))
      .finally(() => setBusy(null));
  };
  const download = (backup: Backup) =>
    run(`download ${backup.name}`, async () =>
      save(await downloadBackup(account.serverUrl, token, backup.name), backup.name),
    );
  const restored = () =>
    setNotice('Restoring: the server restarts, and this page reconnects when it is back.');

  if (!data) {
    return (
      <Card title="Backups">
        <InlineError message={status.error ? describeError(status.error) : null} />
      </Card>
    );
  }
  return (
    <Card title="Backups">
      <Stack gap="md">
        <Facts
          rows={[
            ['Kept in', data.dir],
            ['Schedule', scheduleLabel(data.schedule, offset())],
            [
              'Last scheduled',
              data.last_run
                ? data.last_run.error
                  ? `${at(data.last_run.at)}: failed, ${data.last_run.error}`
                  : `${at(data.last_run.at)}: ${data.last_run.backup}`
                : 'none since the server started',
            ],
          ]}
        />
        <InlineError message={error} />
        {notice ? <Text>{notice}</Text> : null}

        <Text variant="label" muted>
          Back up now
        </Text>
        <Text variant="caption" muted>
          The whole server: its database, artwork, pairings and logins. Without a passphrase, keep
          the file as private as the server itself. A passphrase is needed to restore it, and is
          kept nowhere.
        </Text>
        <Stack horizontal gap="sm">
          <TextField
            label="Passphrase (optional)"
            value={passphrase}
            onChangeText={setPassphrase}
            secureTextEntry
            autoCapitalize="none"
          />
          <Button
            title={data.making ? 'Backing up…' : 'Back up and download'}
            icon="download"
            loading={busy === 'create' || data.making}
            onPress={() =>
              run('create', async () => {
                const made = await createBackup(connection, passphrase || undefined);
                setPassphrase('');
                save(await downloadBackup(account.serverUrl, token, made.name), made.name);
              })
            }
          />
        </Stack>

        <BackupList
          backups={data.backups}
          busy={busy}
          onDownload={download}
          onRestore={(backup) => {
            const secret = backup.encrypted ? window.prompt(`Passphrase for ${backup.name}`) : '';
            if (secret === null || !window.confirm(RESTORE_WARNING)) return;
            run(`restore ${backup.name}`, async () => {
              await restoreStoredBackup(connection, backup.name, secret || undefined);
              restored();
            });
          }}
          onDelete={(backup) => {
            if (!window.confirm(`Delete ${backup.name}?`)) return;
            run(`delete ${backup.name}`, () => deleteBackup(connection, backup.name));
          }}
        />

        <ScheduleForm
          key={JSON.stringify(data.schedule)}
          status={data}
          saving={busy === 'schedule'}
          onSave={(schedule, secret) =>
            run('schedule', () => setBackupSchedule(connection, schedule, secret))
          }
        />

        <RestoreFromFile
          busy={busy === 'upload'}
          onRestore={(file, secret) => {
            if (!window.confirm(RESTORE_WARNING)) return;
            run('upload', async () => {
              await restoreBackupFile(account.serverUrl, token, file, secret || undefined);
              restored();
            });
          }}
        />
      </Stack>
    </Card>
  );
}

function BackupList({
  backups,
  busy,
  onDownload,
  onRestore,
  onDelete,
}: {
  backups: Backup[];
  busy: string | null;
  onDownload: (backup: Backup) => void;
  onRestore: (backup: Backup) => void;
  onDelete: (backup: Backup) => void;
}) {
  if (backups.length === 0) return <Text muted>No backups are kept on the server.</Text>;
  return (
    <View>
      {backups.map((backup) => (
        <View key={backup.name} style={styles.row}>
          <View style={styles.rowText}>
            <Text>{at(backup.created)}</Text>
            <Text variant="caption" muted numberOfLines={1}>
              {`${formatBytes(backup.size)} · ${backup.name}`}
            </Text>
          </View>
          {backup.encrypted ? <Badge label="encrypted" tone="good" /> : null}
          {backup.scheduled ? <Badge label="scheduled" /> : null}
          <Button
            title="Download"
            variant="secondary"
            loading={busy === `download ${backup.name}`}
            onPress={() => onDownload(backup)}
          />
          <Button
            title="Restore"
            variant="secondary"
            loading={busy === `restore ${backup.name}`}
            onPress={() => onRestore(backup)}
          />
          <Button
            title="Delete"
            variant="danger"
            loading={busy === `delete ${backup.name}`}
            onPress={() => onDelete(backup)}
          />
        </View>
      ))}
    </View>
  );
}

type Every = 'off' | 'daily' | 'weekly';

/** The schedule, in local time; the server keeps it in UTC. */
function ScheduleForm({
  status,
  saving,
  onSave,
}: {
  status: BackupsStatus;
  saving: boolean;
  onSave: (
    schedule: { every: 'daily' | 'weekly'; hour: number; weekday: number; keep: number } | null,
    passphrase: string | undefined,
  ) => void;
}) {
  const current = status.schedule;
  const local = current ? toLocal(current, offset()) : { hour: 3, weekday: 0 };
  const [every, setEvery] = useState<Every>(current?.every ?? 'off');
  const [hour, setHour] = useState(local.hour);
  const [weekday, setWeekday] = useState(local.weekday);
  const [keep, setKeep] = useState(String(current?.keep ?? 7));
  const [encrypt, setEncrypt] = useState(status.encrypted);
  const [passphrase, setPassphrase] = useState('');
  const keeping = Number.parseInt(keep, 10);
  const valid = every === 'off' || (keeping >= 1 && (!encrypt || status.encrypted || passphrase));

  return (
    <Stack gap="sm">
      <Text variant="label" muted>
        Schedule
      </Text>
      <Stack horizontal gap="sm">
        <Choice
          title={every === 'off' ? 'Off' : every === 'daily' ? 'Daily' : 'Weekly'}
          label="How often"
          options={[
            { value: 'off', label: 'Off' },
            { value: 'daily', label: 'Daily' },
            { value: 'weekly', label: 'Weekly' },
          ]}
          onChoose={(value) => setEvery(value as Every)}
        />
        {every === 'weekly' ? (
          <Choice
            title={`${WEEKDAYS[weekday]}s`}
            label="On"
            options={WEEKDAYS.map((day, index) => ({ value: String(index), label: day }))}
            onChoose={(value) => setWeekday(Number(value))}
          />
        ) : null}
        {every !== 'off' ? (
          <Choice
            title={`at ${String(hour).padStart(2, '0')}:00`}
            label="At"
            options={Array.from({ length: 24 }, (_, h) => ({
              value: String(h),
              label: `${String(h).padStart(2, '0')}:00`,
            }))}
            onChoose={(value) => setHour(Number(value))}
          />
        ) : null}
      </Stack>
      {every !== 'off' ? (
        <Stack horizontal gap="sm">
          <TextField
            label="Keep the newest"
            value={keep}
            onChangeText={setKeep}
            keyboardType="number-pad"
          />
          <Choice
            title={encrypt ? 'Encrypted' : 'Not encrypted'}
            label="Encryption"
            options={[
              { value: 'yes', label: 'Encrypted' },
              { value: 'no', label: 'Not encrypted' },
            ]}
            onChoose={(value) => setEncrypt(value === 'yes')}
          />
          {encrypt ? (
            <TextField
              label={status.encrypted ? 'New passphrase (optional)' : 'Passphrase'}
              value={passphrase}
              onChangeText={setPassphrase}
              secureTextEntry
              autoCapitalize="none"
            />
          ) : null}
        </Stack>
      ) : null}
      {every !== 'off' && encrypt ? (
        <Text variant="caption" muted>
          Scheduled backups need their passphrase on the server to be made, so it is kept in its
          database, which the backups themselves keep encrypted.
        </Text>
      ) : null}
      <Stack horizontal gap="sm">
        <Button
          title="Save schedule"
          variant="secondary"
          disabled={!valid}
          loading={saving}
          onPress={() => {
            if (every === 'off') return onSave(null, undefined);
            const utc = toUtc({ hour, weekday }, offset());
            onSave(
              { every, hour: utc.hour, weekday: utc.weekday, keep: keeping },
              // Left out keeps the passphrase there is; empty stops encrypting.
              encrypt ? passphrase || undefined : '',
            );
          }}
        />
      </Stack>
    </Stack>
  );
}

function RestoreFromFile({
  busy,
  onRestore,
}: {
  busy: boolean;
  onRestore: (file: File, passphrase: string) => void;
}) {
  const { theme } = useUnistyles();
  const [file, setFile] = useState<File | null>(null);
  const [passphrase, setPassphrase] = useState('');
  return (
    <Stack gap="sm">
      <Text variant="label" muted>
        Restore from a file
      </Text>
      <input
        type="file"
        accept=".zst,.age"
        aria-label="Backup file"
        style={{ color: theme.colors.text, fontFamily: 'system-ui, sans-serif' }}
        onChange={(event) => setFile(event.currentTarget.files?.[0] ?? null)}
      />
      <Stack horizontal gap="sm">
        {file?.name.endsWith('.age') ? (
          <TextField
            label="Passphrase"
            value={passphrase}
            onChangeText={setPassphrase}
            secureTextEntry
            autoCapitalize="none"
          />
        ) : null}
        <Button
          title="Restore"
          variant="danger"
          disabled={!file}
          loading={busy}
          onPress={() => file && onRestore(file, passphrase)}
        />
      </Stack>
    </Stack>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    flexWrap: 'wrap',
    gap: theme.spacing.sm,
    paddingVertical: theme.spacing.xs,
    borderBottomWidth: 1,
    borderColor: theme.colors.border,
  },
  rowText: { flexGrow: 1, flexBasis: 200, minWidth: 0 },
}));
