import { updateEndpointSettings, updateSync } from '@bopsie/client/api/endpoints/admin';
import type { SyncView } from '@bopsie/client/api/schemas/admin';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { useSync } from '@/data/admin';
import { describeError } from '@/lib/errors';
import { IconButton } from '@/ui/controls';
import { Badge, Button, Card, InlineError, Page, Stack, Text, TextField } from '@/ui/primitives';
import { useToast } from '@/ui/Toast';
import { describeCalibrated, ms, parseMs } from './sync';

type Endpoint = SyncView['endpoints'][number];

const at = (unixSeconds: number) => new Date(unixSeconds * 1000).toLocaleString();

/** Runs a change, then reads sync and the devices again; says what went wrong. */
function useChange() {
  const { account } = useConnection();
  const queryClient = useQueryClient();
  const { show } = useToast();
  return async (change: () => Promise<unknown>, said: string) => {
    try {
      await change();
      show(said);
      await queryClient.invalidateQueries({ queryKey: queryKeys.admin(account.id, 'sync') });
      await queryClient.invalidateQueries({ queryKey: queryKeys.admin(account.id, 'devices') });
    } catch (e) {
      show(describeError(e), 'error');
    }
  };
}

/** A delay in milliseconds to type, or to move by steps; saved as it is left. */
function Delay({
  label,
  value,
  onSave,
}: {
  label: string;
  value: number;
  onSave: (value: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const parsed = parseMs(draft);
  const commit = () => {
    if (parsed === null) {
      setDraft(String(value));
    } else if (parsed !== value) {
      onSave(parsed);
    }
  };
  return (
    <Stack horizontal gap="xs" style={styles.delay}>
      <Button title="−5" variant="secondary" onPress={() => onSave(value - 5)} />
      <Button title="−1" variant="secondary" onPress={() => onSave(value - 1)} />
      <TextField
        label=""
        accessibilityLabel={label}
        value={draft}
        keyboardType="numbers-and-punctuation"
        onChangeText={setDraft}
        onSubmitEditing={commit}
        onBlur={commit}
        style={styles.input}
      />
      <Text muted>ms</Text>
      <Button title="+1" variant="secondary" onPress={() => onSave(value + 1)} />
      <Button title="+5" variant="secondary" onPress={() => onSave(value + 5)} />
    </Stack>
  );
}

function EndpointRow({ endpoint }: { endpoint: Endpoint }) {
  const { connection } = useConnection();
  const change = useChange();
  const save = (value: number | null) =>
    change(
      () =>
        updateEndpointSettings(connection, endpoint.endpoint_id, {
          quirks: { sync_adjust_ms: value },
        }),
      value === null
        ? `${endpoint.name} plays without its own adjustment from the next play`
        : `${endpoint.name} waits ${value} ms from the next play`,
    );
  const layer = endpoint.source?.layer ?? 'default';
  return (
    <View style={styles.row}>
      <View style={styles.name}>
        <Text>{endpoint.name}</Text>
        <Stack horizontal gap="xs">
          <Badge label={endpoint.provider} />
          {endpoint.available ? null : <Badge label="away" tone="warning" />}
          {endpoint.sync_adjust_ms !== 0 ? (
            <Badge label={ms(endpoint.sync_adjust_ms)} tone="accent" />
          ) : null}
          {layer !== 'default' && layer !== 'saved' ? <Badge label={layer} /> : null}
        </Stack>
      </View>
      <Stack horizontal gap="xs">
        <Delay
          label={`${endpoint.name} over ${endpoint.provider}, in ms`}
          value={endpoint.sync_adjust_ms}
          onSave={(value) => void save(value)}
        />
        {layer === 'saved' ? (
          <IconButton
            icon="rotate-ccw"
            label={`Clear ${endpoint.name}'s adjustment`}
            variant="ghost"
            size="sm"
            onPress={() => void save(null)}
          />
        ) : null}
      </Stack>
    </View>
  );
}

export function SyncScreen() {
  const { connection } = useConnection();
  const sync = useSync();
  const change = useChange();
  const view = sync.data;
  const last = view?.last_calibration;
  const [showAll, setShowAll] = useState(false);
  const endpoints = (view?.endpoints ?? []).filter(
    (e) => showAll || e.sync_adjust_ms !== 0 || e.available,
  );

  return (
    <Page
      title="Speaker sync"
      subtitle="How much later each speaker plays than the rest of its group. Changes apply from the next play."
    >
      <InlineError message={sync.error ? describeError(sync.error) : null} />
      {view ? (
        <Card title="AirPlay beside other protocols">
          <Text muted>
            AirPlay speakers keep in step with each other on their own clock. In a group that also
            plays over Sendspin or Cast, every AirPlay speaker waits this much more, so the two
            kinds play together. Groups of AirPlay speakers alone play without it.
          </Text>
          <Stack horizontal gap="xs">
            <Delay
              label="AirPlay offset beside other protocols, in ms"
              value={view.mixed_airplay_ms}
              onSave={(value) =>
                void change(
                  () => updateSync(connection, value),
                  `AirPlay speakers wait ${value} ms beside other protocols`,
                )
              }
            />
            {view.mixed_airplay_ms !== 0 ? (
              <IconButton
                icon="rotate-ccw"
                label="Clear the AirPlay offset"
                variant="ghost"
                size="sm"
                onPress={() =>
                  void change(
                    () => updateSync(connection, 0),
                    'AirPlay speakers wait nothing beside other protocols',
                  )
                }
              />
            ) : null}
          </Stack>
        </Card>
      ) : null}
      {view ? (
        <Card title="Speakers">
          <Text muted>
            Each speaker's own adjustment, over each way it is played, in any group. Positive waits;
            negative plays earlier, as far as the group's lead allows.
          </Text>
          {endpoints.map((endpoint) => (
            <EndpointRow key={endpoint.endpoint_id} endpoint={endpoint} />
          ))}
          <Button
            title={showAll ? 'Hide speakers that are away' : 'Show speakers that are away'}
            variant="secondary"
            onPress={() => setShowAll(!showAll)}
          />
        </Card>
      ) : null}
      {view ? (
        <Card title="Last calibration">
          {last ? (
            <Stack gap="xs">
              <Text muted>{at(last.at)}, with Bopsie on a phone.</Text>
              {last.speakers.map((speaker) => (
                <View key={speaker.endpoint_id} style={styles.row}>
                  <Text style={styles.name}>{speaker.name ?? speaker.endpoint_id}</Text>
                  <Stack horizontal gap="xs">
                    {speaker.shared_airplay ? <Badge label="AirPlay offset" tone="accent" /> : null}
                    <Text variant="mono">{describeCalibrated(speaker)}</Text>
                  </Stack>
                </View>
              ))}
            </Stack>
          ) : (
            <Text muted>
              None saved yet. Bopsie on Android calibrates a group from its Account tab: each
              speaker chirps in turn and the phone listens.
            </Text>
          )}
        </Card>
      ) : null}
    </Page>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: { xs: 'column', md: 'row' },
    gap: theme.spacing.sm,
    alignItems: { xs: 'stretch', md: 'center' },
    borderTopWidth: 1,
    borderColor: theme.colors.border,
    paddingTop: theme.spacing.sm,
  },
  name: { width: { xs: 'auto', md: 220 }, gap: theme.spacing.xs },
  delay: { alignItems: 'center', flexWrap: 'wrap' },
  input: { width: 80 },
}));
