import { uploadArtwork } from './admin';

test('artwork is posted raw with the token, and its proxy id comes back', async () => {
  const calls: Array<{ url: string; init: RequestInit | undefined }> = [];
  const fetcher = (async (url: string, init?: RequestInit) => {
    calls.push({ url, init });
    return new Response(JSON.stringify({ proxy_id: 'abc' }), { status: 200 });
  }) as typeof fetch;
  const image = new Blob([new Uint8Array([1, 2, 3])], { type: 'image/png' });
  expect(await uploadArtwork('http://music.local:8095/', 'token', image, fetcher)).toBe('abc');
  const [call] = calls;
  expect(call?.url).toBe('http://music.local:8095/artwork');
  expect(call?.init?.method).toBe('POST');
  const headers = (call?.init?.headers ?? {}) as Record<string, string>;
  expect(headers.Authorization).toBe('Bearer token');
});

test('a refused upload says why', async () => {
  const fetcher = (async () =>
    new Response('Artwork is a JPEG or PNG image', { status: 415 })) as unknown as typeof fetch;
  await expect(uploadArtwork('http://m', 't', new Blob([]), fetcher)).rejects.toThrow(
    'Artwork is a JPEG or PNG image',
  );
});
