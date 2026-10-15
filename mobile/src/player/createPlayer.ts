import type { LocalPlayer } from './Player';

/** Playing on this device is Android-only for now; see the roadmap. */
export function createPlayer(): LocalPlayer {
  return {
    supported: false,
    clientId: () => null,
    connect: async () => {},
    disconnect: async () => {},
    subscribe: (listener) => {
      listener('unsupported');
      return () => {};
    },
    showRemote: () => {},
    onRemoteCommand: () => () => {},
    recordCalibration: async () => null,
    serverNowUs: async () => null,
  };
}
