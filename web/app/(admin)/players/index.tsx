import { useDevices, useGroups, usePlayers } from '@/data/admin';
import { playerRows } from '@/features/players/model';
import { PlayerCard } from '@/features/players/PlayerCard';
import { describeError } from '@/lib/errors';
import { InlineError, Page, Text } from '@/ui/primitives';

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
        <PlayerCard key={row.id} row={row} rows={rows} />
      ))}
    </Page>
  );
}
