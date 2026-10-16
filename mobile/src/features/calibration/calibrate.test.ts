import { delaysOf, describeAdjustment, describeResults, onServerClock } from './calibrate';
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

describe('onServerClock', () => {
  it("takes the recording's times from the server's clock when the phone's is off", () => {
    const recording = {
      sampleRate: 48_000,
      samples: new Int16Array(0),
      firstSampleServerUs: 50_000_000,
    };
    // The phone thought the server's clock 20 s ahead of what the server said it was.
    expect(onServerClock(recording, 120_000_000, 100_000_000).firstSampleServerUs).toBe(30_000_000);
    const unknown = { ...recording, firstSampleServerUs: null };
    expect(onServerClock(unknown, 1, 0).firstSampleServerUs).toBeNull();
  });
});

describe('describeAdjustment', () => {
  it('says what a speaker waits, and what it waited before', () => {
    expect(describeAdjustment('Philips', { previous_ms: 477, sync_adjust_ms: 474 })).toBe(
      'Philips: waits 474 ms (now 477 ms)',
    );
    expect(describeAdjustment('Living Room', { previous_ms: 0, sync_adjust_ms: 0 })).toBe(
      'Living Room: waits 0 ms, as it does now',
    );
  });
});
