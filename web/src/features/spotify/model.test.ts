import { callbackUrl } from './model';

test('the callback is on the server the web UI reaches', () => {
  expect(callbackUrl('https://bopsie.example')).toBe('https://bopsie.example/spotify/callback');
  expect(callbackUrl('http://localhost:8095/')).toBe('http://localhost:8095/spotify/callback');
});
