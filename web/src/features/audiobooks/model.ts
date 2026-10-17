import type { Audiobook, Chapter } from '@bopsie/client/api/schemas/media';
import { uptime } from '@/lib/time';

/** The speeds offered, as Audiobookshelf's own apps offer them. */
export const SPEEDS = [0.75, 1, 1.1, 1.2, 1.25, 1.5, 1.75, 2, 2.5, 3] as const;

export const speedLabel = (speed: number) => `${Number(speed.toFixed(2))}×`;

/** The sleep timers offered, in minutes. */
export const SLEEP_MINUTES = [5, 15, 30, 45, 60, 90] as const;

/** What the sleep timer says on its button: when it pauses, or that there is none. */
export function sleepLabel(
  queue: { sleep_at?: number | null | undefined; sleep_end_of_chapter: boolean },
  nowMs: number,
): string {
  if (queue.sleep_end_of_chapter) return 'Sleep: end of chapter';
  if (queue.sleep_at == null) return 'Sleep';
  const left = Math.max(0, queue.sleep_at - nowMs / 1000);
  // Under a minute it counts the seconds, above it the minutes, rounded up.
  return `Sleep in ${left < 60 ? `${Math.ceil(left)} s` : uptime(Math.ceil(left / 60) * 60)}`;
}

/** How far into a book listening is: finished, how long is left, or nothing when not started. */
export function bookProgress(
  book: Pick<Audiobook, 'fully_played' | 'resume_position_ms' | 'duration'>,
): string | null {
  if (book.fully_played) return 'Finished';
  if (book.resume_position_ms == null || book.duration <= 0) return null;
  const heard = book.resume_position_ms / 1000;
  const percent = Math.floor((heard / book.duration) * 100);
  return `${percent}% · ${uptime(book.duration - heard)} left`;
}

/** How long a chapter runs, in seconds, its end being the next one's start or the book's end. */
export function chapterLength(chapters: Chapter[], index: number, bookSeconds: number): number {
  const chapter = chapters[index];
  if (!chapter) return 0;
  const end = chapter.end ?? chapters[index + 1]?.start ?? bookSeconds;
  return Math.max(0, end - chapter.start);
}
