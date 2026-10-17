import { dropIndex, makeRoom, moveItem } from './reorder';

const rows = [0, 50, 100, 150].map((top) => ({ top, height: 50 }));

test('a dragged row drops past the rows whose middle it has passed', () => {
  expect(dropIndex(rows, 1, 0, 0, 3)).toBe(1);
  expect(dropIndex(rows, 1, 20, 0, 3)).toBe(1);
  expect(dropIndex(rows, 1, 60, 0, 3)).toBe(2);
  expect(dropIndex(rows, 1, 110, 0, 3)).toBe(3);
  expect(dropIndex(rows, 2, -60, 0, 3)).toBe(1);
  expect(dropIndex(rows, 2, -500, 0, 3)).toBe(0);
});

test('a dragged row stays out of what the player holds', () => {
  expect(dropIndex(rows, 3, -500, 2, 3)).toBe(2);
  expect(dropIndex(rows, 2, 500, 2, 3)).toBe(3);
});

test('the rows between where a row was and where it drops stand aside for it', () => {
  expect([0, 1, 2, 3].map((index) => makeRoom(index, 1, 3, 50))).toEqual([0, 0, -50, -50]);
  expect([0, 1, 2, 3].map((index) => makeRoom(index, 3, 1, 50))).toEqual([0, 50, 50, 0]);
});

test('an item moves to where it was dropped', () => {
  expect(moveItem(['a', 'b', 'c', 'd'], 1, 3)).toEqual(['a', 'c', 'd', 'b']);
  expect(moveItem(['a', 'b', 'c', 'd'], 3, 0)).toEqual(['d', 'a', 'b', 'c']);
});
