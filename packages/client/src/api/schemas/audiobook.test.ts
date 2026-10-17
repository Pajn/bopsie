import { audiobook } from './media';
import { playerQueue } from './player';

test('an audiobook reads with its authors, narrators, chapters and progress', () => {
  const book = audiobook.parse({
    item_id: 'li_h',
    provider: 'audiobookshelf',
    name: 'The Hobbit',
    uri: 'audiobookshelf://audiobook/li_h',
    media_type: 'audiobook',
    authors: [
      {
        item_id: 'aut2',
        provider: 'audiobookshelf',
        name: 'J. R. R. Tolkien',
        uri: 'audiobookshelf://artist/aut2',
        media_type: 'artist',
      },
    ],
    narrators: ['Nora Narrator'],
    publisher: null,
    duration: 7201,
    fully_played: false,
    resume_position_ms: 600250,
    metadata: {
      images: [],
      chapters: [
        { position: 0, name: 'One', start: 0, end: 3600.25 },
        { position: 1, name: 'Two', start: 3600.25, end: 7201 },
      ],
    },
  });
  expect(book.authors[0]?.name).toBe('J. R. R. Tolkien');
  expect(book.metadata.chapters[1]?.start).toBe(3600.25);
  expect(book.resume_position_ms).toBe(600250);
});

test("a queue reads its book's speed, chapter and sleep timer, and defaults without them", () => {
  const base = { queue_id: 'kitchen', state: 'playing' };
  const plain = playerQueue.parse(base);
  expect(plain.playback_speed).toBe(1);
  expect(plain.sleep_end_of_chapter).toBe(false);
  expect(plain.sleep_at ?? null).toBeNull();
  const book = playerQueue.parse({
    ...base,
    playback_speed: 1.5,
    current_chapter: 3,
    sleep_at: 1_700_000_000.5,
    sleep_end_of_chapter: false,
  });
  expect([book.playback_speed, book.current_chapter, book.sleep_at]).toEqual([
    1.5, 3, 1_700_000_000.5,
  ]);
});
