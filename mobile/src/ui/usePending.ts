import { useState } from 'react';

/**
 * A press that starts work taking a while, such as playing an artist: `pending` until it settles,
 * so the control can say so, with presses meanwhile ignored rather than starting it again.
 * `work` returns nothing when it had nothing to start.
 */
export function usePending(work: () => Promise<unknown> | undefined) {
  const [pending, setPending] = useState(false);
  const onPress = () => {
    if (pending) return;
    const running = work();
    if (!running) return;
    setPending(true);
    const done = () => setPending(false);
    running.then(done, done);
  };
  return { pending, onPress };
}
