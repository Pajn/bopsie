import { ScrollView } from 'react-native';
import { useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import JsonTree from '@/debug/JsonTree';
import { Card, Facts, Stack, Text } from '@/ui/primitives';

export default function ServerScreen() {
  const { connection } = useConnection();
  const info = connection.serverInfo;
  const { theme } = useUnistyles();
  const colors = {
    text: theme.colors.text,
    muted: theme.colors.textMuted,
    string: theme.colors.code.string,
    value: theme.colors.code.value,
  };
  return (
    <ScrollView>
      <Stack gap="lg">
        <Text variant="title">Server</Text>
        <Card title={info?.name ?? 'Server'}>
          <Facts
            rows={[
              ['Version', info?.server_version ?? '–'],
              ['Schema', String(info?.schema_version ?? '–')],
              ['Server id', info?.server_id ?? '–'],
            ]}
          />
        </Card>
        <Card title="Server info">
          <JsonTree value={JSON.parse(JSON.stringify(info))} colors={colors} />
        </Card>
      </Stack>
    </ScrollView>
  );
}
