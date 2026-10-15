import { searchResults } from '@bopsie/client/api/schemas/media';
import { RECENT_LIMIT, remember, topResult } from './searchResults';

const item = (kind: string, id: string, name: string, provider = 'library') => ({
  item_id: id,
  provider,
  name,
  uri: `${provider}://${kind}/${id}`,
  media_type: kind,
});

const results = (overrides: object) =>
  searchResults.parse({ artists: [], albums: [], tracks: [], playlists: [], ...overrides });

describe('topResult', () => {
  it('takes what is named exactly as searched, the library first', () => {
    const top = topResult(
      results({
        artists: [item('artist', 's', 'Sia', 'spotify'), item('artist', '1', 'Sia')],
        tracks: [item('track', '2', 'Siamese')],
      }),
      'sia',
    );
    expect(top).toMatchObject({ kind: 'artist', item: { item_id: '1' } });
  });

  it('takes an exact album over a loose artist', () => {
    const top = topResult(
      results({ artists: [item('artist', '1', 'Lady A')], albums: [item('album', '7', 'Ocean')] }),
      'Ocean',
    );
    expect(top).toMatchObject({ kind: 'album', item: { item_id: '7' } });
  });

  it('falls back to the first artist, then the first album', () => {
    expect(
      topResult(results({ artists: [item('artist', '1', 'Avantasia')] }), 'sia'),
    ).toMatchObject({ kind: 'artist' });
    expect(topResult(results({ albums: [item('album', '7', 'Fantasia')] }), 'sia')).toMatchObject({
      kind: 'album',
    });
  });

  it('is null with no results', () => {
    expect(topResult(results({}), 'sia')).toBeNull();
  });
});

describe('remember', () => {
  it('puts a search first, once, ignoring case and spaces', () => {
    expect(remember(['metal', 'Sia'], ' sia ')).toEqual(['sia', 'metal']);
  });

  it('ignores an empty search', () => {
    expect(remember(['sia'], '  ')).toEqual(['sia']);
  });

  it('keeps no more than the limit', () => {
    const many = Array.from({ length: RECENT_LIMIT }, (_, i) => `q${i}`);
    expect(remember(many, 'new')).toHaveLength(RECENT_LIMIT);
    expect(remember(many, 'new')[0]).toBe('new');
  });
});
