import { greeting } from './greeting';

test('greets by the hour of the day', () => {
  expect([2, 8, 13, 21].map(greeting)).toEqual([
    'Good night',
    'Good morning',
    'Good afternoon',
    'Good evening',
  ]);
});
