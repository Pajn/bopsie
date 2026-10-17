/** Where a row lies in its list: its top, from the list's own top, and its height. */
export interface RowBox {
  top: number;
  height: number;
}

/**
 * The index the row at `from` drops at when dragged `offset` down (or up, if negative): past
 * every row whose middle its own middle has passed, and kept between `min` and `max`.
 */
export function dropIndex(
  rows: readonly RowBox[],
  from: number,
  offset: number,
  min: number,
  max: number,
): number {
  const dragged = rows[from];
  if (!dragged) return from;
  const middle = dragged.top + dragged.height / 2 + offset;
  let to = from;
  rows.forEach((row, index) => {
    const passed = row.top + row.height / 2;
    if (index > from && middle > passed) to = Math.max(to, index);
    if (index < from && middle < passed) to = Math.min(to, index);
  });
  return Math.min(Math.max(to, min), max);
}

/**
 * How far the row at `index` stands aside while the one at `from` is dragged over to `to`: by
 * the dragged row's height, towards where it was.
 */
export function makeRoom(index: number, from: number, to: number, height: number): number {
  if (from < to && index > from && index <= to) return -height;
  if (to < from && index >= to && index < from) return height;
  return 0;
}

/** `items` with the one at index `from` moved to index `to`. */
export function moveItem<T>(items: readonly T[], from: number, to: number): T[] {
  const moved = items.slice();
  const [item] = moved.splice(from, 1);
  if (item !== undefined) moved.splice(to, 0, item);
  return moved;
}
