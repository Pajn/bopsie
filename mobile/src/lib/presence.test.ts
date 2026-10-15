import { type Presence, withLeaving } from './presence';

const key = (item: string) => item;
const show = (...items: string[]): Presence<string>[] =>
  items.map((item) => ({ key: item, item, present: true, arriving: false }));
const summary = (entries: Presence<string>[]) =>
  entries.map((e) => `${e.key}${e.present ? '' : '-'}${e.arriving ? '+' : ''}`);

describe('withLeaving', () => {
  it('keeps an item that has gone where it was, leaving', () => {
    expect(summary(withLeaving(show('a', 'b', 'c'), ['a', 'c'], key))).toEqual(['a', 'b-', 'c']);
  });

  it('marks an item new to the list as arriving', () => {
    expect(summary(withLeaving(show('a'), ['a', 'b'], key))).toEqual(['a', 'b+']);
  });

  it('keeps several leaving items in their order', () => {
    expect(summary(withLeaving(show('a', 'b', 'c', 'd'), ['a', 'd'], key))).toEqual([
      'a',
      'b-',
      'c-',
      'd',
    ]);
  });

  it('brings a leaving item back when it returns, not as arriving', () => {
    const leaving = withLeaving(show('a', 'b'), ['a'], key);
    expect(summary(withLeaving(leaving, ['a', 'b'], key))).toEqual(['a', 'b']);
  });

  it('keeps a last item that has gone at the end', () => {
    expect(summary(withLeaving(show('a', 'b'), ['a'], key))).toEqual(['a', 'b-']);
  });
});
