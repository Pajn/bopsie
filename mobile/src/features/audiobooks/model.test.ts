import { playerQueue } from '@bopsie/client/api/schemas/player';
import { bookProgress, chapterName, playsABook, sleepLeft, speedLabel } from './model';

const book = {
  queue_id: 'kitchen',
  queue_item_id: 'q1',
  name: 'The Hobbit',
  media_item: {
    item_id: 'li_h',
    provider: 'audiobookshelf',
    name: 'The Hobbit',
    uri: 'audiobookshelf://audiobook/li_h',
    media_type: 'audiobook',
    metadata: {
      images: [],
      chapters: [
        { position: 0, name: 'An Unexpected Party', start: 0, end: 600 },
        { position: 1, name: '', start: 600, end: 1200 },
      ],
    },
  },
};

test('a queue playing a book names its chapter', () => {
  const queue = playerQueue.parse({ queue_id: 'kitchen', current_item: book, current_chapter: 0 });
  expect(playsABook(queue)).toBe(true);
  expect(chapterName(queue)).toBe('An Unexpected Party');
  expect(chapterName({ ...queue, current_chapter: 1 })).toBe('Chapter 2');
  expect(chapterName({ ...queue, current_chapter: null })).toBeNull();
  expect(playsABook(playerQueue.parse({ queue_id: 'kitchen' }))).toBe(false);
});

test('the sleep timer says how long is left', () => {
  const now = 1_000_000_000;
  const at = (seconds: number | null, end = false) => ({
    sleep_at: seconds === null ? null : now / 1000 + seconds,
    sleep_end_of_chapter: end,
  });
  expect(sleepLeft(at(null), now)).toBeNull();
  expect(sleepLeft(at(754.2), now)).toBe('12:35 left');
  expect(sleepLeft(at(null, true), now)).toBe('End of chapter');
});

test('speeds and progress read shortly', () => {
  expect(speedLabel(1.25)).toBe('1.25×');
  expect(speedLabel(2)).toBe('2×');
  expect(bookProgress({ fully_played: false, resume_position_ms: 900_000, duration: 3600 })).toBe(
    '25% · 45:00 left',
  );
  expect(bookProgress({ fully_played: true, resume_position_ms: null, duration: 3600 })).toBe(
    'Finished',
  );
  expect(bookProgress({ fully_played: null, resume_position_ms: null, duration: 3600 })).toBeNull();
});
