import { localKeyValueStore, localSecretStore } from './webStores';

beforeEach(() => localStorage.clear());

test('plain keys and secrets share local storage without seeing each other', async () => {
  localKeyValueStore.set('auth.accounts', '[]');
  await localSecretStore.set('auth.token.x', 'secret');
  localStorage.setItem('someone-else', '1');

  expect(localKeyValueStore.keys()).toEqual(['auth.accounts']);
  expect(localKeyValueStore.get('auth.accounts')).toBe('[]');
  expect(await localSecretStore.get('auth.token.x')).toBe('secret');

  localKeyValueStore.remove('auth.accounts');
  await localSecretStore.remove('auth.token.x');
  expect(localKeyValueStore.keys()).toEqual([]);
  expect(await localSecretStore.get('auth.token.x')).toBeNull();
});
