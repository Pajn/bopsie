import type { KeyValueStore, SecretStore } from '@bopsie/client/auth/storage';

const PREFIX = 'bopsie.';
const SECRET_PREFIX = 'bopsie.secret.';

/**
 * The browser's local storage. The token lives there too: the page is served by the server it
 * signs in to, so no other origin can read it.
 */
export const localKeyValueStore: KeyValueStore = {
  get: (key) => localStorage.getItem(PREFIX + key),
  set: (key, value) => localStorage.setItem(PREFIX + key, value),
  remove: (key) => localStorage.removeItem(PREFIX + key),
  keys: () =>
    Object.keys(localStorage)
      .filter((key) => key.startsWith(PREFIX) && !key.startsWith(SECRET_PREFIX))
      .map((key) => key.slice(PREFIX.length)),
};

export const localSecretStore: SecretStore = {
  get: async (key) => localStorage.getItem(SECRET_PREFIX + key),
  set: async (key, value) => localStorage.setItem(SECRET_PREFIX + key, value),
  remove: async (key) => localStorage.removeItem(SECRET_PREFIX + key),
};
