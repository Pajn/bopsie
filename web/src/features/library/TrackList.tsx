import type { Track } from '@bopsie/client/api/schemas/media';
import { Link } from 'expo-router';
import { useState } from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { usePlayers } from '@/data/admin';
import { duration } from '@/lib/time';
import { IconButton } from '@/ui/controls';
import { Icon } from '@/ui/Icon';
import { Button, Text } from '@/ui/primitives';
import { itemHref } from './ItemRow';
import { PlayButtons, usePlay } from './PlayButtons';

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
  const [open, setOpen] = useState(false);
  // Inside an album or playlist, it plays from this track on; elsewhere, the track alone.
  const { play } = usePlay(from ?? track.uri, from ? track.item_id : undefined);
  const by =
    detail === 'artists'
      ? track.artists.map((artist) => artist.name).join(', ')
      : detail === 'album'
        ? (track.album?.name ?? '')
        : '';
  const title = (
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
  );
  return (
    <View
      style={styles.item(hovered)}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
    >
      <View style={styles.row}>
        <View style={styles.number}>
          {playing ? (
            <Icon name="volume-2" size={14} color={theme.colors.accent} />
          ) : (
            <Text variant="caption" muted style={styles.numberText}>
              {number}
            </Text>
          )}
        </View>
        {/* Where there is room, the title opens the track; play and add sit beside it. */}
        <View style={[styles.grow, styles.wide]}>
          <Link href={itemHref(track) as never} asChild>
            <Pressable accessibilityRole="link">
              {/* Styled inside: the link merges its child's style, which loses unistyles' classes. */}
              {title}
            </Pressable>
          </Link>
        </View>
        {/* On a phone the title plays, as a touch on a track does, and the rest is behind ⋯. */}
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`Play ${track.name}`}
          onPress={() => play('replace')}
          style={[styles.grow, styles.narrow]}
        >
          {title}
        </Pressable>
        <Text variant="caption" muted style={styles.duration}>
          {track.duration ? duration(track.duration) : ''}
        </Text>
        <View style={styles.wide}>
          <PlayButtons uri={from ?? track.uri} {...(from && { startItem: track.item_id })} />
        </View>
        <View style={styles.narrow}>
          <IconButton
            icon="more-horizontal"
            label={open ? `Fewer actions for ${track.name}` : `More actions for ${track.name}`}
            variant="ghost"
            size="sm"
            active={open}
            onPress={() => setOpen(!open)}
          />
        </View>
      </View>
      {open ? (
        <View style={[styles.actions, styles.narrow]}>
          <Button
            title="Add to queue"
            icon="plus"
            variant="secondary"
            onPress={() => {
              play('add');
              setOpen(false);
            }}
          />
          <Link href={itemHref(track) as never} asChild>
            <Pressable accessibilityRole="link">
              <Text style={styles.open}>Open track</Text>
            </Pressable>
          </Link>
        </View>
      ) : null}
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  item: (hovered: boolean) => ({
    paddingHorizontal: theme.spacing.sm,
    marginHorizontal: -theme.spacing.sm,
    borderRadius: theme.radius.md,
    backgroundColor: hovered ? theme.colors.surfaceVariant : 'transparent',
  }),
  row: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.md, minHeight: 44 },
  number: { width: 22, alignItems: 'flex-end' },
  numberText: { fontVariant: ['tabular-nums'] },
  playing: { color: theme.colors.accent, fontWeight: '600' },
  text: { flex: 1, minWidth: 0, paddingVertical: theme.spacing.xs },
  grow: { flex: 1, minWidth: 0 },
  wide: { display: { xs: 'none', md: 'flex' } },
  narrow: { display: { xs: 'flex', md: 'none' } },
  // At least a track's width, so the column lines up; an hour or more widens it, unwrapped.
  duration: {
    display: { xs: 'none', md: 'flex' },
    minWidth: 44,
    textAlign: 'right',
    fontVariant: ['tabular-nums'],
    whiteSpace: 'nowrap',
  } as object,
  actions: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'flex-end',
    gap: theme.spacing.lg,
    paddingBottom: theme.spacing.sm,
  },
  open: { color: theme.colors.accent, fontWeight: '600' },
}));
