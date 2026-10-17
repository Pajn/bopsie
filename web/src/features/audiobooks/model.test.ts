import { bookProgress, chapterLength, sleepLabel, speedLabel } from './model';

describe('speedLabel', () => {
  it('drops the zeros a speed does not need', () => {
    expect(speedLabel(1)).toBe('1×');
    expect(speedLabel(1.25)).toBe('1.25×');
    expect(speedLabel(1.5)).toBe('1.5×');
  });
});

describe('sleepLabel', () => {
  const now = 1_000_000_000;
  it('says when the timer pauses', () => {
    expect(sleepLabel({ sleep_at: null, sleep_end_of_chapter: false }, now)).toBe('Sleep');
    expect(sleepLabel({ sleep_at: now / 1000 + 25.2, sleep_end_of_chapter: false }, now)).toBe(
      'Sleep in 26 s',
    );
    expect(
      sleepLabel({ sleep_at: now / 1000 + 14 * 60 + 5, sleep_end_of_chapter: false }, now),
    ).toBe('Sleep in 15 min');
    expect(sleepLabel({ sleep_at: null, sleep_end_of_chapter: true }, now)).toBe(
      'Sleep: end of chapter',
    );
  });
});

describe('bookProgress', () => {
  it('says how far in and how long is left', () => {
    expect(bookProgress({ fully_played: null, resume_position_ms: null, duration: 3600 })).toBe(
      null,
    );
    expect(bookProgress({ fully_played: true, resume_position_ms: 10, duration: 3600 })).toBe(
      'Finished',
    );
    expect(bookProgress({ fully_played: false, resume_position_ms: 900_000, duration: 3600 })).toBe(
      '25% · 45 min left',
    );
  });
});

describe('chapterLength', () => {
  const chapters = [
    { position: 0, name: 'One', start: 0, end: null },
    { position: 1, name: 'Two', start: 600, end: null },
  ];
  it('ends a chapter at the next one, and the last at the end of the book', () => {
    expect(chapterLength(chapters, 0, 1800)).toBe(600);
    expect(chapterLength(chapters, 1, 1800)).toBe(1200);
    expect(chapterLength([{ position: 0, name: 'One', start: 0, end: 300 }], 0, 1800)).toBe(300);
  });
});
