import type { KeyValueStore, SecretStore } from './storage';
import type { Account } from './types';

const ACCOUNTS_KEY = 'auth.accounts';
const ACTIVE_KEY = 'auth.activeAccountId';

/** Secure-store keys may only contain letters, digits, `.`, `-` and `_`. */
function tokenKey(accountId: string): string {
  let hash = 0;
  for (let i = 0; i < accountId.length; i++) {
    hash = (hash * 31 + accountId.charCodeAt(i)) | 0;
  }
  return `auth.token.${(hash >>> 0).toString(36)}`;
}

/**
 * Persists accounts and the active selection in plain storage, and each
 * account's token in the secret store. Pure over the two store interfaces so
 * it is testable.
 */
export class AccountRepository {
  constructor(
    private readonly kv: KeyValueStore,
    private readonly secrets: SecretStore,
  ) {}

  listAccounts(): Account[] {
    const raw = this.kv.get(ACCOUNTS_KEY);
    if (!raw) return [];
    try {
      const parsed = JSON.parse(raw) as unknown;
      return Array.isArray(parsed) ? (parsed as Account[]) : [];
    } catch {
      return [];
    }
  }

  getActiveAccountId(): string | null {
    return this.kv.get(ACTIVE_KEY);
  }

  setActiveAccountId(id: string | null): void {
    if (id === null) this.kv.remove(ACTIVE_KEY);
    else this.kv.set(ACTIVE_KEY, id);
  }

  /** Inserts or replaces by id. */
  saveAccount(account: Account): void {
    const others = this.listAccounts().filter((existing) => existing.id !== account.id);
    this.kv.set(ACCOUNTS_KEY, JSON.stringify([...others, account]));
  }

  async removeAccount(id: string): Promise<void> {
    const remaining = this.listAccounts().filter((existing) => existing.id !== id);
    this.kv.set(ACCOUNTS_KEY, JSON.stringify(remaining));
    if (this.getActiveAccountId() === id) this.setActiveAccountId(null);
    await this.secrets.remove(tokenKey(id));
  }

  loadToken(id: string): Promise<string | null> {
    return this.secrets.get(tokenKey(id));
  }

  saveToken(id: string, token: string): Promise<void> {
    return this.secrets.set(tokenKey(id), token);
  }
}
