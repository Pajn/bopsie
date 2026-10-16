import { setVolume } from '@bopsie/client/api/endpoints/players';
import { Link } from 'expo-router';
import { useState } from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { IconButton, VolumeSlider } from '@/ui/controls';
import { Icon } from '@/ui/Icon';
import { Badge, Text } from '@/ui/primitives';
import { useRun } from '@/ui/Toast';
import { healthTone, type PlayerRow } from './model';
import { PlayerControls } from './PlayerControls';

/**
 * A player that plays nothing, in a line: its name and protocols, its volume where there is
 * room, and the rest of its controls a press away. A player that cannot be reached is the
 * line alone.
 */
export function PlayerLine({
  row,
  rows,
  last,
}: {
  row: PlayerRow;
  rows: PlayerRow[];
  last: boolean;
}) {
  const { connection } = useConnection();
  const { theme } = useUnistyles();
  const [open, setOpen] = useState(false);
  const run = useRun();
  return (
    <View style={[styles.line, !last && styles.divided, !row.available && styles.unavailable]}>
      <View style={styles.row}>
        <Icon name="speaker" size={18} color={theme.colors.textMuted} />
        <View style={styles.name}>
          <Link href={`/players/${encodeURIComponent(row.id)}`} asChild>
            <Pressable accessibilityRole="link">
              <Text numberOfLines={1} style={styles.link}>
                {row.name}
              </Text>
            </Pressable>
          </Link>
          <Text variant="caption" muted numberOfLines={1}>
            {row.available ? row.protocols.join(' · ') : 'Unavailable'}
          </Text>
        </View>
        {row.health === 'good' || !row.available ? null : (
          <Badge label={row.health} tone={healthTone(row.health)} />
        )}
        {row.available ? (
          <>
            <View style={styles.volume}>
              <VolumeSlider
                value={row.player?.volume_level ?? null}
                muted={row.player?.volume_muted ?? null}
                label={row.name}
                onChange={(level) => run(() => setVolume(connection, row.id, level))}
                onMute={(muted) =>
                  run(() =>
                    connection.command('players/cmd/volume_mute', { player_id: row.id, muted }),
                  )
                }
              />
            </View>
            <IconButton
              icon={open ? 'chevron-up' : 'chevron-down'}
              label={open ? `Fewer controls for ${row.name}` : `More controls for ${row.name}`}
              variant="ghost"
              active={open}
              onPress={() => setOpen(!open)}
            />
          </>
        ) : null}
      </View>
      {open ? (
        <View style={styles.more}>
          <PlayerControls row={row} rows={rows} />
        </View>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  line: { paddingHorizontal: theme.spacing.lg, paddingVertical: theme.spacing.sm },
  divided: { borderBottomWidth: 1, borderColor: theme.colors.border },
  unavailable: { opacity: 0.6 },
  row: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.md, minHeight: 44 },
  name: { flex: 1, minWidth: 0 },
  link: { color: theme.colors.accent, fontWeight: '600' },
  // The volume is in the controls a press away on a phone, where a line has no room for it.
  volume: { display: { xs: 'none', md: 'flex' }, width: 280 },
  more: { paddingTop: theme.spacing.sm, paddingLeft: { xs: 0, md: 18 + theme.spacing.md } },
}));
