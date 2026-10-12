import { playerQueue } from '@/api/schemas/player';
import { nowPlaying } from './nowPlaying';

const queue = (currentItem: object | null) =>
  playerQueue.parse({ queue_id: 'q', current_item: currentItem });

describe('nowPlaying', () => {
  it('is null without a current item', () => {
    expect(nowPlaying(queue(null))).toBeNull();
  });

  it('names the track with its artists and album', () => {
    const result = nowPlaying(
      queue({
        queue_id: 'q',
        queue_item_id: 'i',
        name: 'Artist - Song',
        duration: 181,
        media_item: {
          item_id: '1',
          provider: 'library',
          name: 'Song',
          uri: 'library://track/1',
          media_type: 'track',
          artists: [
            {
              item_id: 'a',
              provider: 'library',
              name: 'Ada',
              uri: 'library://artist/a',
              media_type: 'artist',
            },
            {
              item_id: 'b',
              provider: 'library',
              name: 'Bo',
              uri: 'library://artist/b',
              media_type: 'artist',
            },
          ],
          album: {
            item_id: 'x',
            provider: 'library',
            name: 'Record',
            uri: 'library://album/x',
            media_type: 'album',
          },
        },
      }),
    );
    expect(result).toMatchObject({ title: 'Song', subtitle: 'Ada, Bo · Record', duration: 181 });
  });

  it('falls back to the queue item name for media that is not a track', () => {
    const result = nowPlaying(queue({ queue_id: 'q', queue_item_id: 'i', name: 'Radio One' }));
    expect(result).toMatchObject({ title: 'Radio One', subtitle: '', duration: null });
  });
});
