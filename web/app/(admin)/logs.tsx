import { adminLogs } from '@bopsie/client/api/endpoints/admin';
import type { LogLine } from '@bopsie/client/api/schemas/admin';
import { useEffect, useRef, useState } from 'react';
import { View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import LogView from '@/debug/LogView';
import { append, LEVELS, type LevelName, lineText, visible } from '@/features/logs/model';
import { describeError } from '@/lib/errors';
import { Button, InlineError, Stack, Text, TextField } from '@/ui/primitives';

const POLL_MS = 1_000;

export default function LogsScreen() {
  const { connection } = useConnection();
  const { theme } = useUnistyles();
  const [lines, setLines] = useState<LogLine[]>([]);
  const [level, setLevel] = useState<LevelName>('INFO');
  const [filter, setFilter] = useState('');
  const [paused, setPaused] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const last = useRef(0);

  useEffect(() => {
    if (paused) return;
    let stopped = false;
    const read = async () => {
      try {
        const incoming = await adminLogs(connection, last.current);
        if (stopped) return;
        setError(null);
        if (incoming.length > 0) {
          last.current = incoming.at(-1)?.seq ?? last.current;
          setLines((existing) => append(existing, incoming));
        }
      } catch (e) {
        if (!stopped) setError(describeError(e));
      }
    };
    void read();
    const timer = setInterval(() => void read(), POLL_MS);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [connection, paused]);

  const shown = visible(lines, level, filter).map((line) => ({
    seq: line.seq,
    time: new Date(line.unix_ms).toLocaleTimeString(),
    level: line.level,
    target: line.target,
    text: lineText(line),
  }));

  return (
    <View style={styles.page}>
      <Stack gap="md">
        <Text variant="title">Logs</Text>
        <Stack horizontal gap="xs">
          {LEVELS.slice(0, 4).map((name) => (
            <Button
              key={name}
              title={name}
              variant={name === level ? 'primary' : 'secondary'}
              onPress={() => setLevel(name)}
            />
          ))}
          <Button
            title={paused ? 'Resume' : 'Pause'}
            variant="secondary"
            onPress={() => setPaused(!paused)}
          />
          <Text muted>
            {shown.length} of {lines.length} lines
          </Text>
        </Stack>
        <TextField label="Filter" value={filter} onChangeText={setFilter} autoCorrect={false} />
        <Text variant="caption" muted>
          The server keeps what its log prints: lines below its level (RUST_LOG) never reach here.
        </Text>
        <InlineError message={error} />
        <LogView
          lines={shown}
          colors={{
            text: theme.colors.text,
            muted: theme.colors.textMuted,
            error: theme.colors.danger,
            warn: theme.colors.warning,
            background: theme.colors.surface,
          }}
        />
      </Stack>
    </View>
  );
}

const styles = StyleSheet.create({ page: { flex: 1 } });
