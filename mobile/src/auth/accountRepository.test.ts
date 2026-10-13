import { AccountRepository } from './accountRepository';
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

describe('AccountRepository', () => {
  it('saves, replaces and lists accounts', () => {
    const repo = new AccountRepository(createMemoryKeyValueStore(), createMemorySecretStore());
    repo.saveAccount(account('a'));
    repo.saveAccount(account('b'));
    repo.saveAccount({ ...account('a'), username: 'renamed' });
    expect(repo.listAccounts().map((a) => a.username)).toEqual(['user-b', 'renamed']);
  });

  it('round-trips the token through the secret store and removes it with the account', async () => {
    const repo = new AccountRepository(createMemoryKeyValueStore(), createMemorySecretStore());
    repo.saveAccount(account('a'));
    repo.setActiveAccountId('a');
    await repo.saveToken('a', 'secret');
    expect(await repo.loadToken('a')).toBe('secret');

    await repo.removeAccount('a');
    expect(repo.listAccounts()).toEqual([]);
    expect(repo.getActiveAccountId()).toBeNull();
    expect(await repo.loadToken('a')).toBeNull();
  });

  it('tolerates corrupt storage', () => {
    const kv = createMemoryKeyValueStore({ 'auth.accounts': '{not json' });
    const repo = new AccountRepository(kv, createMemorySecretStore());
    expect(repo.listAccounts()).toEqual([]);
  });
});
