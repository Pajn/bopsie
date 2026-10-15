import { chosenSort, sortOptions } from './sorts';

describe('sortOptions', () => {
  it('offers albums by year, and every kind by name first', () => {
    expect(sortOptions('albums').map((o) => o.key)).toEqual([
      'sort_name',
      'timestamp_added_desc',
      'year_desc',
      'year',
    ]);
    expect(sortOptions('tracks').map((o) => o.key)).toEqual(['sort_name', 'timestamp_added_desc']);
  });
});

describe('chosenSort', () => {
  it('keeps an order the kind offers', () => {
    expect(chosenSort('albums', 'year_desc').label).toBe('Newest');
  });

  it('falls back to name for nothing chosen, or an order the kind does not offer', () => {
    expect(chosenSort('artists', null).key).toBe('sort_name');
    expect(chosenSort('artists', 'year_desc').key).toBe('sort_name');
  });
});
