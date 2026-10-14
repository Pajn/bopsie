import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

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
