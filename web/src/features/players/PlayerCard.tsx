import { imageUrl } from '@bopsie/client/api/images';
import { Link } from 'expo-router';
import { Pressable, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { useQueue } from '@/data/queues';
import { Artwork } from '@/ui/Artwork';
import { Badge, Card, Stack, Text } from '@/ui/primitives';
import { healthTone, type PlayerRow } from './model';
import { PlayerControls } from './PlayerControls';

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

/**
 * The artwork of what a player plays. The queue's current item carries it, as the player's
 * media does not for what plays from the library.
 */
function useArtwork(row: PlayerRow): string | null {
  const { account } = useConnection();
  const playing = (row.player?.playback_state ?? 'idle') !== 'idle';
  const queue = useQueue(row.player?.active_source ?? row.id).data;
  if (!playing) return null;
  const item = queue?.current_item;
  const fromQueue = item
    ? imageUrl(
        account.serverUrl,
        item.image ? { image: item.image } : (item.media_item ?? { image: null }),
        160,
      )
    : null;
  return fromQueue ?? row.player?.current_media?.image_url ?? null;
}

/**
 * A player as a card: its artwork, what it plays, its protocols and its controls. `linked`,
 * as in a list of players, it leads with its name, which opens its page; on that page, the
 * title already names it.
 */
export function PlayerCard({
  row,
  rows,
  linked = true,
}: {
  row: PlayerRow;
  rows: PlayerRow[];
  linked?: boolean;
}) {
  const state = row.player?.playback_state ?? 'idle';
  const media = row.player?.current_media;
  const artwork = useArtwork(row);
  return (
    <Card style={!row.available && styles.unavailable}>
      <View style={styles.header}>
        <Artwork
          uri={artwork}
          size={linked ? 52 : 72}
          placeholder={row.followers.length > 0 ? 'layers' : 'speaker'}
        />
        <View style={styles.titles}>
          {linked ? (
            <Link href={`/players/${encodeURIComponent(row.id)}`} asChild>
              <Pressable accessibilityRole="link">
                <Text variant="heading" style={styles.name} numberOfLines={1}>
                  {row.name}
                </Text>
              </Pressable>
            </Link>
          ) : null}
          <Text variant={linked ? 'body' : 'heading'} muted={linked} numberOfLines={1}>
            {state === 'playing' ? '▶ ' : ''}
            {doing(row)}
          </Text>
          {!linked && media?.album && state !== 'idle' ? (
            <Text muted numberOfLines={1}>
              {media.album}
            </Text>
          ) : null}
          {row.followers.length > 0 ? (
            <Text variant="caption" muted numberOfLines={1}>
              With {row.followers.join(', ')}
              {row.strategy ? ` · ${row.strategy}` : ''}
            </Text>
          ) : null}
        </View>
        <Stack horizontal gap="xs" style={styles.badges(linked)}>
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

const styles = StyleSheet.create((theme) => ({
  header: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.md, flexWrap: 'wrap' },
  titles: { flex: 1, minWidth: 160, gap: 2 },
  name: { color: theme.colors.accent },
  // Beside the name on a wide screen; below it, from the artwork's edge, on a phone.
  badges: (linked: boolean) => ({
    flexWrap: 'wrap',
    marginLeft: { xs: (linked ? 52 : 72) + theme.spacing.md, md: 0 },
  }),
  unavailable: { opacity: 0.6 },
}));
