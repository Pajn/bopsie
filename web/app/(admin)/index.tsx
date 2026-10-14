import { ScrollView } from 'react-native';
import { useConnection } from '@/connection/ConnectionProvider';
import JsonTree from '@/debug/JsonTree';
import { Card, Facts, Stack, Text } from '@/ui/primitives';

export default function ServerScreen() {
  const { connection } = useConnection();
  const info = connection.serverInfo;
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
          <JsonTree value={JSON.parse(JSON.stringify(info))} />
        </Card>
      </Stack>
    </ScrollView>
  );
}
