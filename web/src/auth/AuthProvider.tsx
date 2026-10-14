import { AccountRepository } from '@bopsie/client/auth/accountRepository';
import {
  type AccountState,
  type AccountStore,
  createAccountStore,
  selectActiveAccount,
} from '@bopsie/client/auth/accountStore';
import type { Account } from '@bopsie/client/auth/types';
import { createContext, type ReactNode, useContext, useMemo, useRef } from 'react';
import { useStore } from 'zustand';
import { localKeyValueStore, localSecretStore } from './webStores';

interface AuthContextValue {
  store: AccountStore;
  repository: AccountRepository;
}

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const value = useRef<AuthContextValue | null>(null);
  if (!value.current) {
    const repository = new AccountRepository(localKeyValueStore, localSecretStore);
    const store = createAccountStore(repository);
    // Reading the stored accounts is synchronous, so the first render can have them.
    store.getState().hydrate();
    value.current = { store, repository };
  }
  return <AuthContext.Provider value={value.current}>{children}</AuthContext.Provider>;
}

export function useAuthContext(): AuthContextValue {
  const context = useContext(AuthContext);
  if (!context) throw new Error('useAuth must be used inside AuthProvider');
  return context;
}

export function useAccounts<T>(selector: (state: AccountState) => T): T {
  return useStore(useAuthContext().store, selector);
}

export function useAccountActions() {
  const { store } = useAuthContext();
  return useMemo(() => {
    const { addAccount, removeAccount, markExpired } = store.getState();
    return { addAccount, removeAccount, markExpired };
  }, [store]);
}

export function useActiveAccount(): Account | null {
  return useAccounts(selectActiveAccount);
}
