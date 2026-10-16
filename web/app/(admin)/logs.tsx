import { adminLogs } from '@bopsie/client/api/endpoints/admin';
import type { LogLine } from '@bopsie/client/api/schemas/admin';
import { useEffect, useRef, useState } from 'react';
import { View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import LogView from '@/debug/LogView';
import { append, type LevelName, lineText, visible } from '@/features/logs/model';
import { describeError } from '@/lib/errors';
import { IconButton, SearchField, Tabs } from '@/ui/controls';
import { InlineError, Page, Text } from '@/ui/primitives';

const POLL_MS = 1_000;

const LEVEL_TABS: ReadonlyArray<{ value: LevelName; label: string }> = [
  { value: 'ERROR', label: 'Errors' },
  { value: 'WARN', label: 'Warnings' },
  { value: 'INFO', label: 'Info' },
  { value: 'DEBUG', label: 'Debug' },
];

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
    <Page
      title="Logs"
      subtitle="What the server prints, as it prints it. Lines below its level (RUST_LOG) never reach here."
      wide
    >
      <Tabs tabs={LEVEL_TABS} value={level} onChange={setLevel} />
      <View style={styles.toolbar}>
        <SearchField label="Filter lines" value={filter} onChange={setFilter} />
        <IconButton
          icon={paused ? 'play' : 'pause'}
          label={paused ? 'Resume' : 'Pause'}
          active={paused}
          onPress={() => setPaused(!paused)}
        />
        <Text variant="caption" muted>
          {paused ? 'Paused · ' : ''}
          {shown.length === lines.length
            ? `${lines.length} lines`
            : `${shown.length} of ${lines.length} lines`}
        </Text>
      </View>
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
    </Page>
  );
}

const styles = StyleSheet.create((theme) => ({
  toolbar: {
    flexDirection: 'row',
    alignItems: 'center',
    flexWrap: 'wrap',
    gap: theme.spacing.md,
  },
}));
