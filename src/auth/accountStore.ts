import { createStore, type StoreApi } from 'zustand/vanilla';
import type { AccountRepository } from './accountRepository';
import type { Account } from './types';

export type AuthStatus = 'loading' | 'signedOut' | 'signedIn';

export interface AccountState {
  status: AuthStatus;
  accounts: Account[];
  activeAccountId: string | null;
  /** Accounts whose token the server rejected. They stay listed so the user can sign in again. */
  expiredAccountIds: string[];
  hydrate: () => void;
  addAccount: (account: Account, token: string) => Promise<void>;
  removeAccount: (id: string) => Promise<void>;
  markExpired: (id: string) => void;
}

export type AccountStore = StoreApi<AccountState>;

export function selectActiveAccount(state: AccountState): Account | null {
  return state.accounts.find((account) => account.id === state.activeAccountId) ?? null;
}

export function createAccountStore(repository: AccountRepository): AccountStore {
  return createStore<AccountState>((set, get) => ({
    status: 'loading',
    accounts: [],
    activeAccountId: null,
    expiredAccountIds: [],

    hydrate: () => {
      const accounts = repository.listAccounts();
      const stored = repository.getActiveAccountId();
      const activeAccountId = accounts.some((a) => a.id === stored)
        ? stored
        : (accounts[0]?.id ?? null);
      if (activeAccountId !== stored) repository.setActiveAccountId(activeAccountId);
      set({ accounts, activeAccountId, status: activeAccountId ? 'signedIn' : 'signedOut' });
    },

    addAccount: async (account, token) => {
      await repository.saveToken(account.id, token);
      repository.saveAccount(account);
      repository.setActiveAccountId(account.id);
      set((state) => ({
        accounts: [...state.accounts.filter((a) => a.id !== account.id), account],
        activeAccountId: account.id,
        expiredAccountIds: state.expiredAccountIds.filter((id) => id !== account.id),
        status: 'signedIn',
      }));
    },

    removeAccount: async (id) => {
      await repository.removeAccount(id);
      const accounts = get().accounts.filter((a) => a.id !== id);
      let activeAccountId = get().activeAccountId;
      if (activeAccountId === id) {
        activeAccountId = accounts[0]?.id ?? null;
        repository.setActiveAccountId(activeAccountId);
      }
      set((state) => ({
        accounts,
        activeAccountId,
        expiredAccountIds: state.expiredAccountIds.filter((expired) => expired !== id),
        status: activeAccountId ? 'signedIn' : 'signedOut',
      }));
    },

    markExpired: (id) => {
      set((state) => ({
        expiredAccountIds: state.expiredAccountIds.includes(id)
          ? state.expiredAccountIds
          : [...state.expiredAccountIds, id],
      }));
    },
  }));
}
