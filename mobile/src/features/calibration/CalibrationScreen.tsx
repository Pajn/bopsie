import { applyCalibration } from '@bopsie/client/api/endpoints/admin';
import type { CalibrationAdjustment } from '@bopsie/client/api/schemas/admin';
import { router } from 'expo-router';
import { useState } from 'react';
import { PermissionsAndroid, Platform, ScrollView, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { useOutputs } from '@/features/outputs/useOutputs';
import { describeError } from '@/lib/errors';
import { Button, InlineError, Screen, Stack, Text } from '@/ui/primitives';
import { delaysOf, describeResults, type Heard, listen } from './calibrate';

type State =
  | { step: 'ready' }
  | { step: 'starting' }
  | { step: 'listening'; seconds: number }
  | { step: 'analysing' }
  | { step: 'heard'; heard: Heard; applying: boolean; error: string | null }
  | { step: 'applied'; heard: Heard; adjustments: CalibrationAdjustment[] };

async function microphoneAllowed(): Promise<boolean> {
  if (Platform.OS !== 'android') return false;
  const result = await PermissionsAndroid.request(PermissionsAndroid.PERMISSIONS.RECORD_AUDIO, {
    title: 'Listen to your speakers',
    message:
      'To put your speakers in step, the phone listens to each play a short sound. Nothing is kept.',
    buttonPositive: 'Allow',
  });
  return result === PermissionsAndroid.RESULTS.GRANTED;
}

/**
 * Puts the active group's speakers in step: each plays a chirp in turn, the phone hears when
 * each arrived, and the server makes the early ones wait.
 */
export function CalibrationScreen() {
  const { connection, player, playerStatus } = useConnection();
  const outputs = useOutputs();
  const group = outputs?.active ?? null;
  const [state, setState] = useState<State>({ step: 'ready' });
  const [error, setError] = useState<string | null>(null);

  const busy =
    state.step === 'starting' || state.step === 'listening' || state.step === 'analysing';
  const grouped = group !== null && group.members.length >= 2;
  const connected = playerStatus === 'connected';

  const start = async () => {
    if (!group) return;
    setError(null);
    if (!(await microphoneAllowed())) {
      setError('The phone needs the microphone to hear the speakers.');
      return;
    }
    setState({ step: 'starting' });
    try {
      const heard = await listen(connection, player, group.id, (ms) => {
        setState({ step: 'listening', seconds: Math.ceil(ms / 1000) });
        setTimeout(
          () => setState((now) => (now.step === 'listening' ? { step: 'analysing' } : now)),
          ms,
        );
      });
      setState({ step: 'heard', heard, applying: false, error: null });
    } catch (failure) {
      setError(describeError(failure));
      setState({ step: 'ready' });
    }
  };

  const apply = async (heard: Heard) => {
    setState({ step: 'heard', heard, applying: true, error: null });
    try {
      const adjustments = await applyCalibration(connection, delaysOf(heard.results));
      setState({ step: 'applied', heard, adjustments });
    } catch (failure) {
      setState({ step: 'heard', heard, applying: false, error: describeError(failure) });
    }
  };

  const nameOf = (heard: Heard, endpointId: string) =>
    heard.results.find((r) => r.endpointId === endpointId)?.name ?? endpointId;

  return (
    <Screen>
      <ScrollView contentContainerStyle={styles.content}>
        <Stack gap="lg">
          <Text muted>
            Each speaker in the group plays a short chirp in turn while the phone listens, and the
            ones that play early are made to wait for the rest. Stand where you usually listen, at a
            volume you can hear clearly, and keep the room quiet. Playback pauses.
          </Text>

          <Stack gap="sm">
            <Text variant="label" muted>
              Speakers
            </Text>
            {grouped ? (
              group.members.map((member) => <Text key={member.id}>{member.name}</Text>)
            ) : (
              <Text>Group at least two speakers in Play on first.</Text>
            )}
          </Stack>

          {!connected && (
            <Text muted>
              This phone needs to be connected as a player, to keep time with the server.
            </Text>
          )}

          {state.step === 'starting' && <Text>Getting the speakers ready…</Text>}
          {state.step === 'listening' && (
            <Text testID="calibration-listening">
              Listening for about {state.seconds} seconds. Keep still and quiet.
            </Text>
          )}
          {state.step === 'analysing' && <Text>Working out what was heard…</Text>}

          {(state.step === 'heard' || state.step === 'applied') && (
            <Stack gap="sm">
              <Text variant="label" muted>
                What the phone heard
              </Text>
              {describeResults(state.heard.results).map((line) => (
                <View key={line.endpointId}>
                  <Text>{line.name}</Text>
                  <Text muted={line.ok}>{line.text}</Text>
                </View>
              ))}
              {state.heard.skipped.length > 0 && (
                <Text muted>
                  Not measured, as they follow the group in their own multiroom:{' '}
                  {state.heard.skipped.join(', ')}.
                </Text>
              )}
            </Stack>
          )}

          {state.step === 'applied' && (
            <Stack gap="sm" testID="calibration-applied">
              <Text variant="label" muted>
                Saved
              </Text>
              {state.adjustments.map((adjustment) => (
                <Text key={adjustment.endpoint_id}>
                  {nameOf(state.heard, adjustment.endpoint_id)}: waits {adjustment.sync_adjust_ms}{' '}
                  ms
                  {adjustment.previous_ms !== adjustment.sync_adjust_ms
                    ? ` (was ${adjustment.previous_ms} ms)`
                    : ''}
                </Text>
              ))}
            </Stack>
          )}

          {error && <InlineError message={error} />}
          {state.step === 'heard' && state.error && <InlineError message={state.error} />}

          {state.step === 'heard' && Object.keys(delaysOf(state.heard.results)).length >= 2 && (
            <Button
              title="Save"
              loading={state.applying}
              onPress={() => void apply(state.heard)}
              testID="calibration-apply"
            />
          )}
          {grouped ? (
            <Button
              title={state.step === 'ready' ? 'Start' : 'Calibrate again'}
              variant={state.step === 'ready' ? 'primary' : 'secondary'}
              loading={busy}
              disabled={busy || !connected || (state.step === 'heard' && state.applying)}
              onPress={() => void start()}
              testID="calibration-start"
            />
          ) : (
            <Button
              title="Choose speakers"
              variant="secondary"
              onPress={() => router.push('/outputs')}
            />
          )}
        </Stack>
      </ScrollView>
    </Screen>
  );
}

const styles = StyleSheet.create((theme) => ({
  content: { paddingVertical: theme.spacing.md },
}));
