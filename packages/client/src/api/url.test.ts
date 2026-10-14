import { normalizeServerUrl, socketUrl } from './url';

describe('normalizeServerUrl', () => {
  it.each([
    ['192.168.1.5', 'http://192.168.1.5:8095'],
    ['music.local:9000', 'http://music.local:9000'],
    ['https://music.example.com/', 'https://music.example.com'],
    ['http://music.example.com', 'http://music.example.com'],
    ['  https://example.com/ma//  ', 'https://example.com/ma'],
  ])('%s becomes %s', (input, expected) => {
    expect(normalizeServerUrl(input)).toBe(expected);
  });
});

describe('socketUrl', () => {
  it('switches the scheme and keeps a path prefix', () => {
    expect(socketUrl('https://example.com/ma', '/ws')).toBe('wss://example.com/ma/ws');
    expect(socketUrl('http://10.0.0.2:8095', '/sendspin')).toBe('ws://10.0.0.2:8095/sendspin');
  });
});
