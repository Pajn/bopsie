import * as SecureStore from 'expo-secure-store';
import { Storage } from 'expo-sqlite/kv-store';
import type { KeyValueStore, SecretStore } from './storage';

/** App-wide key-value store on SQLite. Survives reinstall on neither platform. */
export const sqliteKeyValueStore: KeyValueStore = {
  get: (key) => Storage.getItemSync(key),
  set: (key, value) => Storage.setItemSync(key, value),
  remove: (key) => void Storage.removeItemSync(key),
  keys: () => Storage.getAllKeysSync(),
};

/** Keychain on iOS, Keystore-backed encrypted preferences on Android. */
export const secureSecretStore: SecretStore = {
  get: (key) =>
    SecureStore.getItemAsync(key, { keychainAccessible: SecureStore.AFTER_FIRST_UNLOCK }),
  set: (key, value) =>
    SecureStore.setItemAsync(key, value, { keychainAccessible: SecureStore.AFTER_FIRST_UNLOCK }),
  remove: (key) => SecureStore.deleteItemAsync(key),
};
