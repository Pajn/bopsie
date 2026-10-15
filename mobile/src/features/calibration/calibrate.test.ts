import { delaysOf, describeResults } from './calibrate';
import type { SpeakerResult } from './detect';

const result = (
  endpointId: string,
  delayMs: number | null,
  problem: SpeakerResult['problem'] = null,
) =>
  ({
    endpointId,
    name: endpointId,
    delayMs,
    heard: delayMs === null ? 0 : 3,
    rounds: 3,
    spreadMs: problem === 'unsteady' ? 14.4 : 0.3,
    problem,
  }) satisfies SpeakerResult;

describe('describeResults', () => {
  it('says how far ahead of the latest each speaker plays, and why one cannot be told', () => {
    const lines = describeResults([
      result('kitchen', 40.2),
      result('wiim', 15.4),
      result('tv', 39),
      result('patio', null, 'not-heard'),
      result('phone', null, 'unsteady'),
    ]);
    expect(lines.map((l) => [l.endpointId, l.ok, l.text])).toEqual([
      ['kitchen', true, 'In step with the latest.'],
      ['wiim', true, '25 ms ahead; it will wait.'],
      ['tv', true, 'In step with the latest.'],
      ['patio', false, 'Not heard. Turn it up, or move closer to it.'],
      ['phone', false, 'Heard unevenly, 14 ms apart. Try again somewhere quieter.'],
    ]);
  });
});

describe('delaysOf', () => {
  it('keeps only the speakers that were measured', () => {
    expect(delaysOf([result('kitchen', 40.2), result('patio', null, 'not-heard')])).toEqual({
      kitchen: 40.2,
    });
  });
});
