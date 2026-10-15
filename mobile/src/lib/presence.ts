import { useCallback, useState } from 'react';

/** An item on screen: still in the list, or gone from it but still leaving. */
export interface Presence<T> {
  key: string;
  item: T;
  present: boolean;
  /** New to the list since it was first shown, so it arrives rather than being there. */
  arriving: boolean;
}

/**
 * What to show when the list becomes `next`: its items, with those that have gone from it kept
 * where they were, marked as leaving, until they have left. An item that comes back while
 * leaving is present again.
 */
export function withLeaving<T>(
  shown: Presence<T>[],
  next: T[],
  keyOf: (item: T) => string,
): Presence<T>[] {
  const known = new Set(shown.map((entry) => entry.key));
  const result: Presence<T>[] = next.map((item) => {
    const key = keyOf(item);
    return { key, item, present: true, arriving: !known.has(key) };
  });
  const keys = new Set(result.map((entry) => entry.key));
  shown.forEach((entry, index) => {
    if (keys.has(entry.key)) return;
    result.splice(Math.min(index, result.length), 0, { ...entry, present: false });
  });
  return result;
}

/**
 * `items` as they should be shown while they come and go: see `withLeaving`. Whatever shows
 * a leaving item calls `left` with its key once it has gone. The first items are there from
 * the start, not arriving.
 */
export function usePresence<T>(items: T[], keyOf: (item: T) => string) {
  const [source, setSource] = useState(items);
  const [shown, setShown] = useState<Presence<T>[]>(() =>
    items.map((item) => ({ key: keyOf(item), item, present: true, arriving: false })),
  );
  if (items !== source) {
    // Adjusting state to new props while rendering, as React recommends over an effect.
    setSource(items);
    setShown((current) => withLeaving(current, items, keyOf));
  }
  const left = useCallback(
    (key: string) => setShown((current) => current.filter((e) => e.present || e.key !== key)),
    [],
  );
  return { shown, left };
}
