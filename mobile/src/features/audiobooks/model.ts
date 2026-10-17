import type { Audiobook } from '@bopsie/client/api/schemas/media';
import type { PlayerQueue } from '@bopsie/client/api/schemas/player';
import { formatDuration } from '@/player/progress';

/** The speeds offered, as shelfie offers them. */
export const SPEEDS = [0.75, 1, 1.1, 1.2, 1.25, 1.5, 1.75, 2, 2.5, 3] as const;

export const speedLabel = (speed: number) => `${Number(speed.toFixed(2))}×`;

/** The sleep timers offered, in minutes. */
export const SLEEP_MINUTES = [5, 15, 30, 45, 60, 90] as const;

/** Whether what a queue plays now is an audiobook. */
export const playsABook = (queue: Pick<PlayerQueue, 'current_item'> | null | undefined) =>
  queue?.current_item?.media_item?.media_type === 'audiobook';

/** Whether the queue's sleep timer is set. */
export const sleeping = (queue: Pick<PlayerQueue, 'sleep_at' | 'sleep_end_of_chapter'>) =>
  queue.sleep_at != null || queue.sleep_end_of_chapter;

/** When the sleep timer pauses, said shortly: `12:30 left`, `End of chapter`, or nothing. */
export function sleepLeft(
  queue: Pick<PlayerQueue, 'sleep_at' | 'sleep_end_of_chapter'>,
  nowMs: number,
): string | null {
  if (queue.sleep_end_of_chapter) return 'End of chapter';
  if (queue.sleep_at == null) return null;
  return `${formatDuration(Math.max(0, Math.ceil(queue.sleep_at - nowMs / 1000)))} left`;
}

/** The name of the chapter playing in a queue's book, when it has chapters. */
export function chapterName(
  queue: Pick<PlayerQueue, 'current_item' | 'current_chapter'>,
): string | null {
  const chapters = queue.current_item?.media_item?.metadata.chapters ?? [];
  const index = queue.current_chapter;
  if (index == null) return null;
  const chapter = chapters[index];
  return chapter ? chapter.name || `Chapter ${index + 1}` : null;
}

/** How far into a book listening is: finished, how much is left, or nothing when not started. */
export function bookProgress(
  book: Pick<Audiobook, 'fully_played' | 'resume_position_ms' | 'duration'>,
): string | null {
  if (book.fully_played) return 'Finished';
  if (book.resume_position_ms == null || book.duration <= 0) return null;
  const heard = book.resume_position_ms / 1000;
  return `${Math.floor((heard / book.duration) * 100)}% · ${formatDuration(book.duration - heard)} left`;
}
