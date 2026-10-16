import { Link } from 'expo-router';
import { Pressable, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useDevices, useGroups, usePlayers } from '@/data/admin';
import { healthTone, type PlayerRow, playerRows } from '@/features/players/model';
import { PlayerControls } from '@/features/players/PlayerControls';
import { describeError } from '@/lib/errors';
import { Artwork } from '@/ui/Artwork';
import { Badge, Card, InlineError, Page, Stack, Text } from '@/ui/primitives';

/** What a player is doing, in a line: what it plays, or that it follows or is idle. */
function doing(row: PlayerRow): string {
  const media = row.player?.current_media;
  const state = row.player?.playback_state ?? 'idle';
  if (row.follows) return `Plays along with ${row.follows}`;
  if (media?.title && state !== 'idle') {
    return [media.title, media.artist].filter(Boolean).join(' · ');
  }
  return row.available ? 'Idle' : 'Unavailable';
}

function Row({ row, rows }: { row: PlayerRow; rows: PlayerRow[] }) {
  const state = row.player?.playback_state ?? 'idle';
  const media = row.player?.current_media;
  return (
    <Card style={!row.available && styles.unavailable}>
      <View style={styles.header}>
        <Artwork
          uri={state !== 'idle' ? media?.image_url : null}
          size={52}
          placeholder={row.followers.length > 0 ? 'layers' : 'speaker'}
        />
        <View style={styles.titles}>
          <Link href={`/players/${encodeURIComponent(row.id)}`} asChild>
            <Pressable accessibilityRole="link">
              <Text variant="heading" style={styles.name} numberOfLines={1}>
                {row.name}
              </Text>
            </Pressable>
          </Link>
          <Text muted numberOfLines={1}>
            {state === 'playing' ? '▶ ' : ''}
            {doing(row)}
          </Text>
          {row.followers.length > 0 ? (
            <Text variant="caption" muted numberOfLines={1}>
              With {row.followers.join(', ')}
              {row.strategy ? ` · ${row.strategy}` : ''}
            </Text>
          ) : null}
        </View>
        <Stack horizontal gap="xs" style={styles.badges}>
          {row.protocols.map((protocol) => (
            <Badge key={protocol} label={protocol} tone="accent" />
          ))}
          {row.health === 'good' ? null : (
            <Badge label={row.health} tone={healthTone(row.health)} />
          )}
        </Stack>
      </View>
      {row.available ? <PlayerControls row={row} rows={rows} /> : null}
    </Card>
  );
}

export default function PlayersScreen() {
  const devices = useDevices();
  const players = usePlayers();
  const groups = useGroups();
  const error = devices.error ?? players.error ?? groups.error;
  const rows = playerRows(devices.data ?? [], players.data ?? [], groups.data ?? []);
  return (
    <Page title="Players" subtitle="Every speaker the server knows, and what each plays">
      <InlineError message={error ? describeError(error) : null} />
      {devices.isSuccess && rows.length === 0 ? (
        <Text muted>No players yet. Speakers show here once the server finds them.</Text>
      ) : null}
      {rows.map((row) => (
        <Row key={row.id} row={row} rows={rows} />
      ))}
    </Page>
  );
}

const styles = StyleSheet.create((theme) => ({
  header: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.md, flexWrap: 'wrap' },
  titles: { flex: 1, minWidth: 160, gap: 2 },
  name: { color: theme.colors.accent },
  // Beside the name on a wide screen; below it, from the artwork's edge, on a phone.
  badges: { flexWrap: 'wrap', marginLeft: { xs: 64, md: 0 } },
  unavailable: { opacity: 0.6 },
}));
