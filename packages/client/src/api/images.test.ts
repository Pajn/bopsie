import { imageUrl } from './images';

const image = (extra: object = {}) => ({
  type: 'thumb',
  path: '/music/A/B/cover.jpg',
  provider: 'filesystem_local--x',
  remotely_accessible: false,
  ...extra,
});

describe('imageUrl', () => {
  it('uses the proxy id when the server sends one', () => {
    const item = { metadata: { images: [image({ proxy_id: 'abc' })] } };
    expect(imageUrl('https://music.example.com/', item, 256)).toBe(
      'https://music.example.com/imageproxy/abc?size=256&fmt=jpeg',
    );
  });

  it('prefers the thumb among several images', () => {
    const item = {
      metadata: { images: [image({ type: 'fanart', proxy_id: 'f' }), image({ proxy_id: 't' })] },
    };
    expect(imageUrl('http://ma:8095', item, 80)).toContain('/imageproxy/t?');
  });

  it('falls back to the stream port query form for servers before 2.10', () => {
    const url = imageUrl('http://ma.local:8095', { image: image() }, 160);
    expect(url).toBe(
      'http://ma.local:8097/imageproxy?provider=filesystem_local--x&size=160&fmt=jpeg&path=%252Fmusic%252FA%252FB%252Fcover.jpg',
    );
  });

  it('uses a public image as it is when there is no proxy id', () => {
    const remote = image({ path: 'https://i.scdn.co/image/abc', remotely_accessible: true });
    expect(imageUrl('http://ma:8095', { image: remote }, 256)).toBe('https://i.scdn.co/image/abc');
  });

  it('is null for an item without images', () => {
    expect(imageUrl('http://ma:8095', { metadata: { images: [] } }, 80)).toBeNull();
    expect(imageUrl('http://ma:8095', { image: null }, 80)).toBeNull();
  });
});
