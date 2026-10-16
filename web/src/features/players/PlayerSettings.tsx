import {
  pairFinish,
  pairStart,
  updateEndpointSettings,
  updatePlayerSettings,
} from '@bopsie/client/api/endpoints/admin';
import type { AdminDevice, AdminEndpoint } from '@bopsie/client/api/schemas/admin';
import { queryKeys } from '@bopsie/client/connection/queryKeys';
import { useQueryClient } from '@tanstack/react-query';
import { type ReactNode, useEffect, useRef, useState } from 'react';
import { View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { describeError } from '@/lib/errors';
import { IconButton } from '@/ui/controls';
import { Badge, Button, Card, Choice, InlineError, Stack, Text, TextField } from '@/ui/primitives';
import { describeSource } from './model';

/** Runs a change, then reads the admin views again; says what went wrong. */
function useSave() {
  const { account } = useConnection();
  const queryClient = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const save = async (change: () => Promise<unknown>) => {
    setError(null);
    setBusy(true);
    try {
      await change();
      await queryClient.invalidateQueries({ queryKey: queryKeys.admin(account.id, 'devices') });
      await queryClient.invalidateQueries({ queryKey: queryKeys.players(account.id) });
    } catch (e) {
      setError(describeError(e));
    } finally {
      setBusy(false);
    }
  };
  return { save, error, busy };
}

/**
 * Where a setting's value comes from, when not the default: saved here, the config file or a
 * rule, with a way back from a saved one. A default says nothing, as most settings are.
 */
function Layer({ layer, onReset }: { layer: string; onReset?: () => void }) {
  if (layer === 'default') return null;
  return (
    <Stack horizontal gap="xs">
      <Badge label={layer} tone={layer === 'saved' ? 'accent' : 'muted'} />
      {layer === 'saved' && onReset ? (
        <IconButton
          icon="rotate-ccw"
          label="Reset to what it would be unsaved"
          variant="ghost"
          size="sm"
          onPress={onReset}
        />
      ) : null}
    </Stack>
  );
}

function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string | undefined;
  children: ReactNode;
}) {
  return (
    <View style={styles.row}>
      <View style={styles.label}>
        <Text variant="label">{label}</Text>
        {hint ? (
          <Text variant="caption" muted>
            {hint}
          </Text>
        ) : null}
      </View>
      <View style={styles.value}>{children}</View>
    </View>
  );
}

/**
 * A text field saved on Enter, on leaving it, or with the button that shows once it is changed,
 * so typing does not save every letter. It follows the value when that changes elsewhere.
 */
function SavedField({
  value,
  placeholder,
  secure,
  numeric,
  onSave,
}: {
  value: string;
  placeholder?: string;
  secure?: boolean;
  numeric?: boolean;
  onSave: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  // Pressing Save leaves the field first; what was just saved is not saved again.
  const sent = useRef<string | null>(null);
  const changed = draft !== value;
  const commit = () => {
    if (!changed || sent.current === draft) return;
    sent.current = draft;
    onSave(draft);
  };
  return (
    <Stack horizontal gap="xs">
      <TextField
        label=""
        value={draft}
        placeholder={placeholder ?? ''}
        secureTextEntry={secure ?? false}
        keyboardType={numeric ? 'number-pad' : 'default'}
        onChangeText={setDraft}
        onSubmitEditing={commit}
        onBlur={commit}
        style={styles.input}
      />
      {changed ? <Button title="Save" onPress={commit} /> : null}
    </Stack>
  );
}

const SAMPLE_CAPS = [
  { value: '', label: 'No cap' },
  { value: '44100/16', label: '44.1 kHz / 16-bit' },
  { value: '48000/16', label: '48 kHz / 16-bit' },
  { value: '48000/24', label: '48 kHz / 24-bit' },
];

function quirk(endpoint: AdminEndpoint, name: string): unknown {
  return endpoint.quirks[name] ?? null;
}

function quirkLayer(endpoint: AdminEndpoint, name: string): string {
  return endpoint.quirk_sources[name]?.layer ?? 'default';
}

function EndpointSettings({ endpoint, last }: { endpoint: AdminEndpoint; last: boolean }) {
  const { connection } = useConnection();
  const { save, error, busy } = useSave();
  const [pin, setPin] = useState<string | null>(null);
  const id = endpoint.id;
  const update = (changes: Parameters<typeof updateEndpointSettings>[2]) =>
    save(() => updateEndpointSettings(connection, id, changes));
  const setQuirk = (name: string, value: unknown) => update({ quirks: { [name]: value } });
  const airplay = endpoint.provider === 'airplay';
  const pairing = endpoint.capabilities.pairing;
  const diagnostics = (endpoint.diagnostics ?? {}) as { paired?: boolean };
  const adjust = Number(quirk(endpoint, 'sync_adjust_ms') ?? 0);
  const cap = quirk(endpoint, 'max_sample_format') as { sample_rate: number; bits: number } | null;
  const enabled = endpoint.settings.enabled.value;

  return (
    <Stack gap="sm" style={styles.endpoint}>
      <Text variant="heading">
        {endpoint.provider} · {endpoint.id}
      </Text>
      <Row
        label="Plays over it"
        hint={last && enabled ? 'Its only protocol: off, the player cannot play.' : undefined}
      >
        <Stack horizontal gap="xs">
          <Button
            title={enabled ? 'On' : 'Off'}
            variant={enabled ? 'primary' : 'secondary'}
            loading={busy}
            onPress={() => void update({ enabled: !enabled })}
          />
          <Layer
            layer={endpoint.settings.enabled.layer}
            onReset={() => void update({ enabled: null })}
          />
        </Stack>
      </Row>
      {airplay && (pairing === 'password' || endpoint.settings.password.value) ? (
        <Row label="Password" hint={endpoint.settings.password.value ? 'Set' : 'Not set'}>
          <Stack horizontal gap="xs">
            <SavedField
              value=""
              secure
              placeholder="New password"
              onSave={(password) => void update({ password: password || null })}
            />
            <Layer
              layer={endpoint.settings.password.layer}
              onReset={() => void update({ password: null })}
            />
          </Stack>
        </Row>
      ) : null}
      {airplay && pairing === 'pin' ? (
        <Row label="Pairing" hint={diagnostics.paired ? 'Paired' : 'Not paired: it will not play'}>
          {pin === null ? (
            <Button
              title={diagnostics.paired ? 'Pair again' : 'Pair'}
              variant="secondary"
              onPress={() =>
                void save(async () => {
                  await pairStart(connection, id);
                  setPin('');
                })
              }
            />
          ) : (
            <Stack horizontal gap="xs">
              <TextField
                label=""
                value={pin}
                placeholder="PIN on the speaker"
                keyboardType="number-pad"
                onChangeText={setPin}
                style={styles.input}
              />
              <Button
                title="Finish"
                disabled={pin.length === 0}
                onPress={() =>
                  void save(async () => {
                    await pairFinish(connection, id, pin);
                    setPin(null);
                  })
                }
              />
            </Stack>
          )}
        </Row>
      ) : null}
      <Text variant="label" muted>
        ADVANCED · applies from the next play
      </Text>
      <Row label="Sync adjust" hint="Later (+) or earlier (−) than the rest of a group">
        <Stack horizontal gap="xs">
          <Button
            title="−5"
            variant="secondary"
            onPress={() => void setQuirk('sync_adjust_ms', adjust - 5)}
          />
          <Text variant="mono">{`${adjust} ms`}</Text>
          <Button
            title="+5"
            variant="secondary"
            onPress={() => void setQuirk('sync_adjust_ms', adjust + 5)}
          />
          <Layer
            layer={quirkLayer(endpoint, 'sync_adjust_ms')}
            onReset={() => void setQuirk('sync_adjust_ms', null)}
          />
        </Stack>
      </Row>
      <Row label="Minimum lead" hint="Start playback at least this far ahead, in ms">
        <Stack horizontal gap="xs">
          <SavedField
            numeric
            value={String(quirk(endpoint, 'min_lead_ms') ?? '')}
            placeholder="None"
            onSave={(text) => void setQuirk('min_lead_ms', text === '' ? null : Number(text))}
          />
          <Layer
            layer={quirkLayer(endpoint, 'min_lead_ms')}
            onReset={() => void setQuirk('min_lead_ms', null)}
          />
        </Stack>
      </Row>
      <Row label="Best format" hint="For speakers that play silence on better">
        <Stack horizontal gap="xs">
          <Choice
            title={
              SAMPLE_CAPS.find((c) => c.value === (cap ? `${cap.sample_rate}/${cap.bits}` : ''))
                ?.label ?? 'No cap'
            }
            options={SAMPLE_CAPS}
            onChoose={(value) => {
              const [rate, bits] = value.split('/').map(Number);
              void setQuirk('max_sample_format', value === '' ? null : { sample_rate: rate, bits });
            }}
          />
          <Layer
            layer={quirkLayer(endpoint, 'max_sample_format')}
            onReset={() => void setQuirk('max_sample_format', null)}
          />
        </Stack>
      </Row>
      {airplay ? (
        <>
          <Row label="AirPlay timing">
            <Stack horizontal gap="xs">
              <Choice
                title={String(quirk(endpoint, 'timing') ?? 'auto')}
                options={['auto', 'ptp', 'ntp'].map((v) => ({ value: v, label: v }))}
                onChoose={(value) => void setQuirk('timing', value)}
              />
              <Layer
                layer={quirkLayer(endpoint, 'timing')}
                onReset={() => void setQuirk('timing', null)}
              />
            </Stack>
          </Row>
          <Row label="AirPlay route">
            <Stack horizontal gap="xs">
              <Choice
                title={String(quirk(endpoint, 'protocol') ?? 'auto')}
                options={['auto', 'raop', 'airplay2', 'airplay2-compat'].map((v) => ({
                  value: v,
                  label: v,
                }))}
                onChoose={(value) => void setQuirk('protocol', value)}
              />
              <Layer
                layer={quirkLayer(endpoint, 'protocol')}
                onReset={() => void setQuirk('protocol', null)}
              />
            </Stack>
          </Row>
        </>
      ) : null}
      <InlineError message={error} />
    </Stack>
  );
}

/** A player's settings: saved over the config file, each saying where it comes from. */
export function PlayerSettings({ device }: { device: AdminDevice }) {
  const { connection } = useConnection();
  const { save, error, busy } = useSave();
  const update = (changes: Parameters<typeof updatePlayerSettings>[2]) =>
    save(() => updatePlayerSettings(connection, device.id, changes));
  const settings = device.settings;
  const on = device.members.filter((m) => m.settings.enabled.value);

  return (
    <Card title="Settings">
      <Row label="Name" hint="Instead of the name it gives itself">
        <Stack horizontal gap="xs">
          <SavedField
            value={settings.name.value ?? ''}
            placeholder={device.members[0]?.name ?? device.id}
            onSave={(name) => void update({ name: name.trim() || null })}
          />
          <Layer layer={settings.name.layer} onReset={() => void update({ name: null })} />
        </Stack>
      </Row>
      <Row label="Enabled" hint="A disabled player is hidden and never played to">
        <Stack horizontal gap="xs">
          <Button
            title={settings.enabled.value ? 'Enabled' : 'Disabled'}
            variant={settings.enabled.value ? 'primary' : 'secondary'}
            loading={busy}
            onPress={() => void update({ enabled: !settings.enabled.value })}
          />
          <Layer layer={settings.enabled.layer} onReset={() => void update({ enabled: null })} />
        </Stack>
      </Row>
      <Row label="Maximum volume" hint="The server holds it there, whoever turns it up">
        <Stack horizontal gap="xs">
          <SavedField
            numeric
            value={settings.max_volume.value === null ? '' : String(settings.max_volume.value)}
            placeholder="None"
            onSave={(text) =>
              void update({
                max_volume: text === '' ? null : Math.max(0, Math.min(100, Number(text))),
              })
            }
          />
          <Layer
            layer={settings.max_volume.layer}
            onReset={() => void update({ max_volume: null })}
          />
        </Stack>
      </Row>
      <Row
        label="Spotify Connect"
        hint="Shows in the Spotify app; picking it plays Spotify to this player's group"
      >
        <Stack gap="xs">
          <Stack horizontal gap="xs">
            <Button
              title={settings.spotify_connect.value ? 'On' : 'Off'}
              variant={settings.spotify_connect.value ? 'primary' : 'secondary'}
              loading={busy}
              onPress={() => void update({ spotify_connect: !settings.spotify_connect.value })}
            />
            <Layer
              layer={settings.spotify_connect.layer}
              onReset={() => void update({ spotify_connect: null })}
            />
          </Stack>
          {device.sources.map((source) => (
            <Text key={source.id} variant="caption" muted>
              {describeSource(source)}
            </Text>
          ))}
        </Stack>
      </Row>
      {settings.spotify_connect.value ? (
        <Row label="Spotify name" hint="What it is called in the Spotify app">
          <Stack horizontal gap="xs">
            <SavedField
              value={settings.spotify_name.value ?? ''}
              placeholder={device.spotify_default_name}
              onSave={(name) => void update({ spotify_name: name.trim() || null })}
            />
            <Layer
              layer={settings.spotify_name.layer}
              onReset={() => void update({ spotify_name: null })}
            />
          </Stack>
        </Row>
      ) : null}
      <InlineError message={error} />
      {device.members.map((endpoint) => (
        <EndpointSettings
          key={endpoint.id}
          endpoint={endpoint}
          last={on.length === 1 && on[0]?.id === endpoint.id}
        />
      ))}
    </Card>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: {
    flexDirection: { xs: 'column', md: 'row' },
    gap: theme.spacing.sm,
    alignItems: { xs: 'stretch', md: 'center' },
  },
  label: { width: { xs: 'auto', md: 200 } },
  value: { flex: 1 },
  input: { minWidth: 160 },
  endpoint: {
    borderTopWidth: 1,
    borderColor: theme.colors.border,
    paddingTop: theme.spacing.md,
  },
}));
