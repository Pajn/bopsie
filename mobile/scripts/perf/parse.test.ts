import { gatesFromLog, imagesSettledAt, parseAmStart, parseBenchLines } from './parse.ts';
import { compare, summarize } from './stats.ts';

const APP = 'io.github.pajn.bopsie';

const LOG = [
  '1000.000000 100 200 I ActivityTaskManager: START u0 {flg=0x10000000 cmp=io.github.pajn.bopsie/.MainActivity} from uid 2000',
  '1000.120000 100 200 I ActivityManager: Start proc 4242:io.github.pajn.bopsie/u0a123 for pre-top-activity',
  '1000.600000 4242 4300 I ReactNativeJS: [bench] mark js-entry 12.5',
  '1000.900000 4242 4300 I ReactNativeJS: [bench] mark root-render 312.5',
  '1001.000000 100 200 I ActivityTaskManager: Displayed io.github.pajn.bopsie/.MainActivity for user 0: +1s5ms',
  '1001.100000 4242 4300 I ReactNativeJS: [bench] mark first-screen-commit 512.5',
  '1001.300000 4242 4300 I ReactNativeJS: [bench] mark home-render 712.5',
  '1001.400000 4242 4300 I ReactNativeJS: [bench] mark image 800',
  '1001.450000 4242 4300 I ReactNativeJS: [bench] mark image 850',
  '1002.400000 4242 4300 I ReactNativeJS: [bench] mark image 1800',
  '1001.500000 4242 4300 I ReactNativeJS: [bench] mark home-shelves 912.5',
];

describe('startup bench parsing', () => {
  it('reads marks and images', () => {
    const log = parseBenchLines(LOG);
    expect(log.marks['js-entry']).toBe(12.5);
    expect(log.images).toEqual([800, 850, 1800]);
    expect(imagesSettledAt(log.images)).toBe(850);
  });

  it('places marks on the launch clock and adds the system milestones', () => {
    const gates = gatesFromLog(
      LOG,
      APP,
      'Status: ok\nLaunchState: COLD\nTotalTime: 1005\nWaitTime: 1010',
    );
    // js-entry logged 600 ms after START at runtime time 12.5 → origin 587.5.
    expect(gates).toEqual({
      processStart: 120,
      displayed: 1005,
      totalTime: 1005,
      jsEntry: 600,
      rootRender: 900,
      firstScreenCommit: 1100,
      homeRender: 1300,
      homeShelves: 1500,
      firstImage: 1387.5,
      painted: 1437.5,
    });
  });

  it('reads am start output', () => {
    expect(parseAmStart('LaunchState: WARM\nTotalTime: 300')).toEqual({
      totalTime: 300,
      launchState: 'WARM',
    });
  });
});

describe('stats', () => {
  it('summarises and compares with noise floors', () => {
    const a = summarize([100, 110, 120, 130, 140]);
    const b = summarize([200, 210, 220, 230, 240]);
    expect(a).toEqual({ median: 120, iqr: 20, n: 5 });
    expect(b && a && compare(a, b)).toBe('better');
    expect(a && compare(a, { median: 125, iqr: 20, n: 5 })).toBe('same');
    expect(summarize([])).toBeUndefined();
  });
});
