import type { Track } from '@bopsie/client/api/schemas/media';
import { Link } from 'expo-router';
import { useState } from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { duration } from '@/lib/time';
import { Text } from '@/ui/primitives';
import { itemHref } from './ItemRow';
import { PlayButtons } from './PlayButtons';

/**
 * Tracks as numbered rows. Inside an album or playlist, `from` is its uri, and Play plays it
 * from that track on; elsewhere, as among an artist's tracks, the track alone. `artists` shows
 * who made each, where they differ from track to track.
 */
export function TrackList({
  tracks,
  from,
  numbered = true,
  artists = false,
}: {
  tracks: Track[];
  from?: string | undefined;
  numbered?: boolean;
  artists?: boolean;
}) {
  return (
    <View>
      {tracks.map((track, index) => (
        <TrackRow
          key={track.uri}
          track={track}
          number={numbered ? (track.track_number ?? index + 1) : index + 1}
          from={from}
          artists={artists}
        />
      ))}
    </View>
  );
}

function TrackRow({
  track,
  number,
  from,
  artists,
}: {
  track: Track;
  number: number;
  from: string | undefined;
  artists: boolean;
}) {
  const [hovered, setHovered] = useState(false);
  const by = track.artists.map((artist) => artist.name).join(', ');
  return (
    <View
      style={styles.row(hovered)}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
    >
      <Text variant="caption" muted style={styles.number}>
        {number}
      </Text>
      {/* The link's own element takes no style, so a wrapper takes the width. */}
      <View style={styles.grow}>
        <Link href={itemHref(track) as never} asChild>
          <Pressable accessibilityRole="link">
            {/* Styled inside: the link merges its child's style, which loses unistyles' classes. */}
            <View style={styles.text}>
              <Text numberOfLines={1}>{track.name}</Text>
              {artists && by ? (
                <Text variant="caption" muted numberOfLines={1}>
                  {by}
                </Text>
              ) : null}
            </View>
          </Pressable>
        </Link>
      </View>
      <Text variant="caption" muted style={styles.duration}>
        {track.duration ? duration(track.duration) : ''}
      </Text>
      {from ? (
        <PlayButtons uri={from} startItem={track.item_id} />
      ) : (
        <PlayButtons uri={track.uri} />
      )}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  row: (hovered: boolean) => ({
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    minHeight: 44,
    paddingHorizontal: theme.spacing.sm,
    marginHorizontal: -theme.spacing.sm,
    borderRadius: theme.radius.md,
    backgroundColor: hovered ? theme.colors.surfaceVariant : 'transparent',
  }),
  number: { width: 22, textAlign: 'right', fontVariant: ['tabular-nums'] },
  text: { flex: 1, minWidth: 0, paddingVertical: theme.spacing.xs },
  grow: { flex: 1, minWidth: 0 },
  duration: { width: 44, textAlign: 'right', fontVariant: ['tabular-nums'] },
}));
