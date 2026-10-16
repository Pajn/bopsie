import type { Track } from '@bopsie/client/api/schemas/media';
import { Link } from 'expo-router';
import { useState } from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { usePlayers } from '@/data/admin';
import { duration } from '@/lib/time';
import { Icon } from '@/ui/Icon';
import { Text } from '@/ui/primitives';
import { itemHref } from './ItemRow';
import { PlayButtons } from './PlayButtons';

/**
 * Tracks as numbered rows. Inside an album or playlist, `from` is its uri, and Play plays it
 * from that track on; elsewhere, as among an artist's tracks, the track alone. `detail` is
 * the line under each: who made it, where that differs from track to track, as in a playlist,
 * or its album, among an artist's tracks.
 */
export function TrackList({
  tracks,
  from,
  numbered = true,
  detail = null,
}: {
  tracks: Track[];
  from?: string | undefined;
  numbered?: boolean;
  detail?: 'artists' | 'album' | null;
}) {
  const players = usePlayers();
  // What any player plays now, by uri, to mark it in the list.
  const playing = new Set(
    (players.data ?? [])
      .filter((player) => player.playback_state === 'playing')
      .map((player) => player.current_media?.uri)
      .filter(Boolean),
  );
  return (
    <View>
      {tracks.map((track, index) => (
        <TrackRow
          key={track.uri}
          track={track}
          number={numbered ? (track.track_number ?? index + 1) : index + 1}
          from={from}
          detail={detail}
          playing={playing.has(track.uri)}
        />
      ))}
    </View>
  );
}

function TrackRow({
  track,
  number,
  from,
  detail,
  playing,
}: {
  track: Track;
  number: number;
  from: string | undefined;
  detail: 'artists' | 'album' | null;
  playing: boolean;
}) {
  const { theme } = useUnistyles();
  const [hovered, setHovered] = useState(false);
  const by =
    detail === 'artists'
      ? track.artists.map((artist) => artist.name).join(', ')
      : detail === 'album'
        ? (track.album?.name ?? '')
        : '';
  return (
    <View
      style={styles.row(hovered)}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
    >
      <View style={styles.number}>
        {playing ? (
          <Icon name="volume-2" size={14} color={theme.colors.accent} />
        ) : (
          <Text variant="caption" muted style={styles.numberText}>
            {number}
          </Text>
        )}
      </View>
      {/* The link's own element takes no style, so a wrapper takes the width. */}
      <View style={styles.grow}>
        <Link href={itemHref(track) as never} asChild>
          <Pressable accessibilityRole="link">
            {/* Styled inside: the link merges its child's style, which loses unistyles' classes. */}
            <View style={styles.text}>
              <Text
                numberOfLines={1}
                style={playing && styles.playing}
                // The whole title, where a long one is cut short.
                {...({ title: track.name } as object)}
              >
                {track.name}
              </Text>
              {by ? (
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
  number: { width: 22, alignItems: 'flex-end' },
  numberText: { fontVariant: ['tabular-nums'] },
  playing: { color: theme.colors.accent, fontWeight: '600' },
  text: { flex: 1, minWidth: 0, paddingVertical: theme.spacing.xs },
  grow: { flex: 1, minWidth: 0 },
  duration: { width: 44, textAlign: 'right', fontVariant: ['tabular-nums'] },
}));
