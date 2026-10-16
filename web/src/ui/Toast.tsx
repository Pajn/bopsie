import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
} from 'react';
import { Pressable, View } from 'react-native';
import { StyleSheet, useUnistyles } from 'react-native-unistyles';
import { describeError } from '@/lib/errors';
import { Icon } from './Icon';
import { Text } from './primitives';

type Tone = 'info' | 'error';

interface Toast {
  id: number;
  message: string;
  tone: Tone;
}

interface Toasts {
  /** Says `message` for a few seconds at the foot of the screen; an error stays longer. */
  show: (message: string, tone?: Tone) => void;
}

const ToastContext = createContext<Toasts>({ show: () => {} });

/** What an action says when it is done or has failed, wherever it was taken. */
export function useToast(): Toasts {
  return useContext(ToastContext);
}

/** Runs `action`, saying what went wrong if it fails, and `done` once it succeeds. */
export function useRun() {
  const { show } = useToast();
  return useCallback(
    (action: () => Promise<unknown>, done?: string) => {
      action().then(
        () => done && show(done),
        (e: unknown) => show(describeError(e), 'error'),
      );
    },
    [show],
  );
}

/**
 * Holds the toasts and draws them over the page, above a phone's tab bar. A press dismisses
 * one; a newer one of the same words replaces it rather than stacking.
 */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const next = useRef(0);
  const dismiss = useCallback((id: number) => {
    setToasts((all) => all.filter((toast) => toast.id !== id));
  }, []);
  const show = useCallback(
    (message: string, tone: Tone = 'info') => {
      const id = next.current++;
      setToasts((all) =>
        [...all.filter((toast) => toast.message !== message), { id, message, tone }].slice(-3),
      );
      setTimeout(() => dismiss(id), tone === 'error' ? 8000 : 4000);
    },
    [dismiss],
  );
  const value = useMemo(() => ({ show }), [show]);
  return (
    <ToastContext.Provider value={value}>
      {children}
      <View style={styles.stack} pointerEvents="box-none">
        {toasts.map((toast) => (
          <ToastView key={toast.id} toast={toast} onDismiss={() => dismiss(toast.id)} />
        ))}
      </View>
    </ToastContext.Provider>
  );
}

function ToastView({ toast, onDismiss }: { toast: Toast; onDismiss: () => void }) {
  const { theme } = useUnistyles();
  const error = toast.tone === 'error';
  return (
    <Pressable
      onPress={onDismiss}
      accessibilityRole="alert"
      accessibilityLabel={toast.message}
      style={styles.toast(error)}
    >
      <Icon
        name={error ? 'alert-circle' : 'check'}
        size={16}
        color={error ? theme.colors.danger : theme.colors.background}
      />
      <Text style={styles.message(error)}>{toast.message}</Text>
    </Pressable>
  );
}

const styles = StyleSheet.create((theme) => ({
  stack: {
    position: 'absolute',
    left: 0,
    right: 0,
    // Above a phone's tab bar; near the foot of a wide screen.
    bottom: { xs: 76, md: theme.spacing.xl },
    alignItems: 'center',
    gap: theme.spacing.sm,
    paddingHorizontal: theme.spacing.lg,
  },
  toast: (error: boolean) => ({
    flexDirection: 'row',
    alignItems: 'center',
    gap: theme.spacing.sm,
    maxWidth: 480,
    paddingHorizontal: theme.spacing.lg,
    paddingVertical: theme.spacing.md,
    borderRadius: theme.radius.lg,
    backgroundColor: error ? theme.colors.surface : theme.colors.text,
    borderWidth: error ? 1 : 0,
    borderColor: theme.colors.danger,
    boxShadow: '0 6px 24px rgba(0, 0, 0, 0.18)',
  }),
  message: (error: boolean) => ({
    flexShrink: 1,
    color: error ? theme.colors.text : theme.colors.background,
  }),
}));
