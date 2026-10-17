import { itemHref } from './links';

describe('itemHref', () => {
  it("is the item's library page, its provider and id kept whole", () => {
    expect(itemHref({ media_type: 'album', provider: 'library', item_id: '42' })).toBe(
      '/library/album/library/42',
    );
    expect(itemHref({ media_type: 'artist', provider: 'spotify--a1', item_id: 'x/y' })).toBe(
      '/library/artist/spotify--a1/x%2Fy',
    );
  });
});
