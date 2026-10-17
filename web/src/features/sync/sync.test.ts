import { describeCalibrated, ms, parseMs } from './sync';

describe('ms', () => {
  it('signs a delay', () => {
    expect([ms(12), ms(0), ms(-5)]).toEqual(['+12 ms', '0 ms', '−5 ms']);
  });
});

describe('parseMs', () => {
  it('reads a delay however it is typed', () => {
    expect(['477', '+12', '−5', '-5 ms', ' 3.6 '].map(parseMs)).toEqual([477, 12, -5, -5, 4]);
    expect(['', 'soon', '1e3'].map(parseMs)).toEqual([null, null, null]);
  });
});

describe('describeCalibrated', () => {
  const speaker = {
    endpoint_id: 'philips',
    device_id: 'philips',
    name: 'Philips',
    heard_ms: 12.4,
    previous_ms: 0,
    sync_adjust_ms: 477,
    shared_airplay: true,
  };
  it('says how late it was heard and what it set', () => {
    expect(describeCalibrated(speaker)).toBe('heard 12 ms late; AirPlay offset 477 ms (was 0 ms)');
    expect(describeCalibrated({ ...speaker, shared_airplay: false, previous_ms: 477 })).toBe(
      'heard 12 ms late; waits 477 ms, as before',
    );
  });
});
