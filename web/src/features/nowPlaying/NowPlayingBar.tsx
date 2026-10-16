import { queueCommand, seek, setVolume } from '@bopsie/client/api/endpoints/players';
import { Link } from 'expo-router';
import { useEffect } from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet } from 'react-native-unistyles';
import { useConnection } from '@/connection/ConnectionProvider';
import { useQueue } from '@/data/queues';
import { usePlayTarget } from '@/features/library/target';
import { describeItem, itemArtwork } from '@/features/queues/items';
import { elapsedNow } from '@/lib/time';
import { useNow } from '@/lib/useNow';
import { Artwork } from '@/ui/Artwork';
import { IconButton, SeekBar, VolumeSlider } from '@/ui/controls';
import { Choice, Text } from '@/ui/primitives';
import { useRun } from '@/ui/Toast';

/** Whether a key press belongs to what has focus, as Space does to a field or a button. */
function typing(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  return (
    ['INPUT', 'TEXTAREA', 'SELECT', 'BUTTON', 'A'].includes(target.tagName) ||
    target.getAttribute('role') === 'button' ||
    target.getAttribute('role') === 'link'
  );
}

/**
 * What plays on the player the web controls, at the foot of every page: the same player the
 * library plays to, so picking one in either place picks it in both. Its artwork, what plays and
 * the transport; where there is room, where in it, the volume and the player to choose; on a
 * phone it is compact and opens the queue. Space plays and pauses, unless something else has
 * the key.
 */
export function NowPlayingBar() {
  const { account, connection } = useConnection();
  const { player, leaders, setPlayerId } = usePlayTarget();
  const run = useRun();
  const now = useNow();
  const queueId = player ? (player.active_source ?? player.player_id) : '';
  const queue = useQueue(queueId).data;
  const item = queue?.current_item ?? null;
  const described = item ? describeItem(item) : null;
  const playing = queue?.state === 'playing';
  const elapsed = queue ? elapsedNow(queue, now) : 0;
  const length = item?.duration ?? 0;
  const command = (name: 'previous' | 'play_pause' | 'next') =>
    run(() => queueCommand(connection, queueId, name));

  useEffect(() => {
    if (!player) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.code !== 'Space' || event.repeat || typing(event.target)) return;
      event.preventDefault();
      run(() => queueCommand(connection, queueId, 'play_pause'));
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [player, connection, queueId, run]);

  if (!player) return null;
  const queueHref = `/queues/${encodeURIComponent(queueId)}`;
  return (
    <View style={styles.bar} role="region" aria-label="Now playing">
      {/* A phone has no seek bar; a line along the top says how far in it is. */}
      {item && length > 0 ? (
        <View style={styles.progressTrack}>
          <View
            style={[styles.progress, { width: `${Math.min(100, (elapsed / length) * 100)}%` }]}
          />
        </View>
      ) : null}
      {/* On a phone the whole of what plays opens the queue; where there is room, its title does. */}
      <View style={styles.nowSlot}>
        <Link href={queueHref as never} asChild>
          <Pressable accessibilityRole="link" accessibilityLabel={`${player.name}'s queue`}>
            <View style={styles.now}>
              <Artwork
                uri={item ? itemArtwork(account.serverUrl, item, 80) : null}
                size={44}
                placeholder="music"
              />
              <View style={styles.titles}>
                <Text numberOfLines={1} style={styles.title}>
                  {described?.title ?? 'Nothing playing'}
                </Text>
                <Text variant="caption" muted numberOfLines={1}>
                  {described
                    ? [described.subtitle, player.name].filter(Boolean).join(' · ')
                    : `on ${player.name}`}
                </Text>
              </View>
            </View>
          </Pressable>
        </Link>
      </View>
      <View style={styles.middle}>
        <View style={styles.transport}>
          <View style={styles.wide}>
            <IconButton
              icon="skip-back"
              label="Previous"
              variant="ghost"
              disabled={!item}
              onPress={() => command('previous')}
            />
          </View>
          <IconButton
            icon={playing ? 'pause' : 'play'}
            label={playing ? 'Pause' : 'Play'}
            variant="primary"
            disabled={!item}
            onPress={() => command('play_pause')}
          />
          <IconButton
            icon="skip-forward"
            label="Next"
            variant="ghost"
            disabled={!item}
            onPress={() => command('next')}
          />
        </View>
        {item ? (
          <View style={[styles.seek, styles.wide]}>
            <SeekBar
              elapsed={elapsed}
              length={length}
              onSeek={(seconds) => run(() => seek(connection, queueId, seconds))}
            />
          </View>
        ) : null}
      </View>
      <View style={[styles.end, styles.wide]}>
        <View style={styles.volume}>
          <VolumeSlider
            value={player.volume_level ?? null}
            muted={player.volume_muted ?? null}
            label={player.name}
            onChange={(level) => run(() => setVolume(connection, player.player_id, level))}
            onMute={(muted) =>
              run(() =>
                connection.command('players/cmd/volume_mute', {
                  player_id: player.player_id,
                  muted,
                }),
              )
            }
          />
        </View>
        <Choice
          title={player.name}
          label={`Plays on ${player.name}`}
          options={leaders.map((p) => ({ value: p.player_id, label: p.name }))}
          onChoose={setPlayerId}
        />
      </View>
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  bar: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: { xs: theme.spacing.sm, md: theme.spacing.lg },
    paddingHorizontal: { xs: theme.spacing.md, md: theme.spacing.xl },
    paddingVertical: theme.spacing.sm,
    backgroundColor: theme.colors.surface,
    borderTopWidth: 1,
    borderColor: theme.colors.border,
  },
  // What plays takes what room is left on a phone, and a fixed share on a wide screen; the
  // link's own element takes no style, so a wrapper shares out the width.
  nowSlot: {
    flexGrow: { xs: 1, md: 0 },
    flexShrink: 1,
    width: { xs: 'auto', md: 300 },
    minWidth: 0,
  },
  now: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.md,
    minWidth: 0,
  },
  titles: { flex: 1, minWidth: 0 },
  title: { fontWeight: '600' },
  middle: {
    flexGrow: { xs: 0, md: 1 },
    flexBasis: { xs: 'auto', md: 0 },
    alignItems: 'center',
    gap: 2,
  },
  transport: { flexDirection: 'row', alignItems: 'center', gap: theme.spacing.xs },
  seek: { alignSelf: 'stretch', maxWidth: 520, width: '100%', marginHorizontal: 'auto' },
  end: { alignItems: 'center', flexDirection: 'row', gap: theme.spacing.md },
  volume: { width: 180 },
  wide: { display: { xs: 'none', md: 'flex' } },
  progressTrack: {
    display: { xs: 'flex', md: 'none' },
    position: 'absolute',
    top: -1,
    left: 0,
    right: 0,
    height: 2,
  },
  progress: { height: 2, backgroundColor: theme.colors.accent },
}));
