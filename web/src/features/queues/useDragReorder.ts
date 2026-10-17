import { useCallback, useEffect, useRef, useState } from 'react';
import { dropIndex, type RowBox } from './reorder';

/** A row being dragged: where it was, where it would drop, and how far it has been moved. */
export interface Drag {
  from: number;
  to: number;
  offset: number;
  /** The dragged row's height, which the rows it passes stand aside by. */
  height: number;
}

/**
 * Drags rows of a list into a new order with the pointer, mouse or finger alike. The rows are
 * measured as the drag starts, against the list's own top, so scrolling while dragging keeps
 * the row under the pointer. Rows drop between `min` and `max`; `onDrop` hears where one was
 * let go, unless it is where it started.
 */
export function useDragReorder(
  min: number,
  max: number,
  onDrop: (from: number, to: number) => void,
) {
  // On the web a view is its element.
  const list = useRef<HTMLElement | null>(null);
  const rows = useRef(new Map<number, HTMLElement>());
  const [drag, setDrag] = useState<Drag | null>(null);
  const stop = useRef<(() => void) | null>(null);

  useEffect(() => () => stop.current?.(), []);

  const listRef = useCallback((node: object | null) => {
    list.current = node as unknown as HTMLElement | null;
  }, []);

  const rowRef = useCallback(
    (index: number) => (node: object | null) => {
      if (node) rows.current.set(index, node as unknown as HTMLElement);
      else rows.current.delete(index);
    },
    [],
  );

  const start = (from: number, clientY: number) => {
    const container = list.current;
    if (!container || stop.current) return;
    const listTop = () => container.getBoundingClientRect().top;
    const top = listTop();
    const boxes: RowBox[] = [];
    for (const [index, row] of rows.current) {
      const rect = row.getBoundingClientRect();
      boxes[index] = { top: rect.top - top, height: rect.height };
    }
    const height = boxes[from]?.height ?? 0;
    const startY = clientY - top;
    let to = from;
    setDrag({ from, to, offset: 0, height });

    const move = (event: PointerEvent) => {
      const offset = event.clientY - listTop() - startY;
      to = dropIndex(boxes, from, offset, min, max);
      setDrag({ from, to, offset, height });
    };
    const end = (event: PointerEvent) => {
      stop.current?.();
      if (event.type === 'pointerup' && to !== from) onDrop(from, to);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
    window.addEventListener('pointercancel', end);
    stop.current = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      window.removeEventListener('pointercancel', end);
      stop.current = null;
      setDrag(null);
    };
  };

  return { drag, listRef, rowRef, start };
}
