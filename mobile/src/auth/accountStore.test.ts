import { AccountRepository } from './accountRepository';
import { createAccountStore, selectActiveAccount } from './accountStore';
import { createMemoryKeyValueStore, createMemorySecretStore } from './storage';
import type { Account } from './types';

const account = (id: string): Account => ({
  id,
  serverUrl: 'https://music.example.com',
  serverId: 'server',
  userId: id,
  username: `user-${id}`,
  addedAt: 1,
});

function setup() {
  const repository = new AccountRepository(createMemoryKeyValueStore(), createMemorySecretStore());
  return { repository, store: createAccountStore(repository) };
}

describe('account store', () => {
  it('starts signed out with nothing stored', () => {
    const { store } = setup();
    store.getState().hydrate();
    expect(store.getState().status).toBe('signedOut');
  });

  it('signs in on adding an account and persists it', async () => {
    const { store, repository } = setup();
    store.getState().hydrate();
    await store.getState().addAccount(account('a'), 'token');
    expect(store.getState().status).toBe('signedIn');
    expect(selectActiveAccount(store.getState())?.id).toBe('a');
    expect(await repository.loadToken('a')).toBe('token');
  });

  it('falls back to another account when the active one is removed', async () => {
    const { store } = setup();
    store.getState().hydrate();
    await store.getState().addAccount(account('a'), 'x');
    await store.getState().addAccount(account('b'), 'y');
    await store.getState().removeAccount('b');
    expect(store.getState().activeAccountId).toBe('a');
    await store.getState().removeAccount('a');
    expect(store.getState().status).toBe('signedOut');
  });

  it('clears an expired mark when the account signs in again', async () => {
    const { store } = setup();
    store.getState().hydrate();
    await store.getState().addAccount(account('a'), 'x');
    store.getState().markExpired('a');
    expect(store.getState().expiredAccountIds).toEqual(['a']);
    await store.getState().addAccount(account('a'), 'new');
    expect(store.getState().expiredAccountIds).toEqual([]);
  });
});
