import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import { usePlayers } from '@/data/admin';

interface TargetState {
  /** The player the library plays to, by id. */
  playerId: string | null;
  setPlayerId: (playerId: string) => void;
}

/** Where the library's Play and Add go, remembered across visits. */
export const useTarget = create<TargetState>()(
  persist(
    (set) => ({
      playerId: null,
      setPlayerId: (playerId) => set({ playerId }),
    }),
    { name: 'bopsie.library.target', storage: createJSONStorage(() => localStorage) },
  ),
);

/**
 * The player the library plays to: the one picked, while it can play, else one playing, else
 * the first that can, so Play works before anything is picked. Followers play their leader's
 * queue, so only leaders are offered.
 */
export function usePlayTarget() {
  const players = usePlayers();
  const picked = useTarget((state) => state.playerId);
  const setPlayerId = useTarget((state) => state.setPlayerId);
  const leaders = (players.data ?? [])
    .filter((p) => p.available && !p.synced_to)
    .sort((a, b) => a.name.localeCompare(b.name));
  const player =
    leaders.find((p) => p.player_id === picked) ??
    leaders.find((p) => p.playback_state === 'playing') ??
    leaders[0];
  return { player, leaders, setPlayerId };
}
