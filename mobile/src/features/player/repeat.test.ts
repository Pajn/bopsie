import { nextRepeatMode } from './repeat';

describe('nextRepeatMode', () => {
  it('cycles from off, to the queue, to the track, and back to off', () => {
    expect(nextRepeatMode('off')).toBe('all');
    expect(nextRepeatMode('all')).toBe('one');
    expect(nextRepeatMode('one')).toBe('off');
  });
});
