import { greeting } from './greeting';

test('the greeting follows the hour', () => {
  expect(greeting(3)).toBe('Good night');
  expect(greeting(9)).toBe('Good morning');
  expect(greeting(15)).toBe('Good afternoon');
  expect(greeting(21)).toBe('Good evening');
});
