/**
 * Turns an Android device log into launch gates. The app logs
 * `[bench] mark <name> <ms>` for its milestones (see src/lib/bench.ts); the
 * activity manager's own lines add the process start and first frame.
 */

const MARK = /\[bench\] mark (\S+) (-?\d+(?:\.\d+)?)/;
const LOGCAT_TIME = /^\s*(\d+\.\d+)\s/;

/** How far apart two first-screen images may land and still count as one run. */
const IMAGE_QUIET_MS = 150;

export interface BenchLog {
  /** First occurrence of each mark, ms since the JavaScript runtime started. */
  marks: Record<string, number>;
  /** Every image mark, in order. */
  images: number[];
}

export function parseBenchLines(lines: Iterable<string>): BenchLog {
  const marks: Record<string, number> = {};
  const images: number[] = [];
  for (const line of lines) {
    const match = MARK.exec(line);
    if (!match) continue;
    const name = match[1] as string;
    const value = Number(match[2]);
    if (name === 'image') images.push(value);
    else marks[name] ??= value;
  }
  return { marks, images };
}

/** Gates, all ms from the launch command. */
export interface LaunchGates {
  processStart?: number;
  jsEntry?: number;
  /** The entry file's imports have all evaluated. */
  modulesLoaded?: number;
  /** The root layout module started evaluating. */
  layoutModule?: number;
  rootRender?: number;
  /** The local database is open, and its migrations have finished. */
  dbOpen?: number;
  dbReady?: number;
  /** The stored accounts are in the account store. */
  accountsHydrated?: number;
  /** The active account's token is read from secure storage, so its connection can open. */
  tokenLoaded?: number;
  /** The server's API connection has authenticated, so commands can be sent. */
  connectionReady?: number;
  /** The first screen's covers are decoded and in memory. */
  coversPrefetched?: number;
  firstScreenCommit?: number;
  homeRender?: number;
  homeShelves?: number;
  /** Home's root view has laid out natively: the content is mounted and about to draw. */
  homeMounted?: number;
  firstImage?: number;
  painted?: number;
  /** The activity manager's `Displayed` time. */
  displayed?: number;
  /** `am start -W`'s TotalTime. */
  totalTime?: number;
}

export const GATE_ORDER: (keyof LaunchGates)[] = [
  'processStart',
  'jsEntry',
  'modulesLoaded',
  'layoutModule',
  'rootRender',
  'dbOpen',
  'dbReady',
  'accountsHydrated',
  'tokenLoaded',
  'coversPrefetched',
  'firstScreenCommit',
  'displayed',
  'homeRender',
  'connectionReady',
  'homeShelves',
  'homeMounted',
  'firstImage',
  'painted',
  'totalTime',
];

const MARK_GATES: Record<string, keyof LaunchGates> = {
  'js-entry': 'jsEntry',
  'modules-loaded': 'modulesLoaded',
  'layout-module': 'layoutModule',
  'root-render': 'rootRender',
  'db-open': 'dbOpen',
  'db-ready': 'dbReady',
  'accounts-hydrated': 'accountsHydrated',
  'token-loaded': 'tokenLoaded',
  'connection-ready': 'connectionReady',
  'covers-prefetched': 'coversPrefetched',
  'first-screen-commit': 'firstScreenCommit',
  'home-render': 'homeRender',
  'home-shelves': 'homeShelves',
  'home-mounted': 'homeMounted',
};

/** The last image of the first run of images, or undefined without images. */
export function imagesSettledAt(images: number[]): number | undefined {
  let settled = images[0];
  if (settled === undefined) return undefined;
  for (const at of images.slice(1)) {
    if (at - settled > IMAGE_QUIET_MS) break;
    settled = at;
  }
  return settled;
}

export function logcatTime(line: string): number | undefined {
  const match = LOGCAT_TIME.exec(line);
  return match ? Number(match[1]) * 1000 : undefined;
}

/** Epoch ms of the log line carrying the first `name` mark. */
export function markLineTime(lines: Iterable<string>, name: string): number | undefined {
  for (const line of lines) {
    const match = MARK.exec(line);
    if (match?.[1] !== name) continue;
    return logcatTime(line);
  }
  return undefined;
}

export interface AndroidSystemLog {
  /** Epoch ms of the activity manager's START line: the launch. */
  startAt?: number;
  processStart?: number;
  displayed?: number;
}

export function parseAndroidSystemLog(lines: Iterable<string>, appId: string): AndroidSystemLog {
  const component = `${escapeRegExp(appId)}/\\.MainActivity`;
  const start = new RegExp(`ActivityTaskManager: START u0 .*cmp=${component}`);
  const proc = new RegExp(`ActivityManager: Start proc \\d+:${escapeRegExp(appId)}/`);
  const displayed = new RegExp(
    `ActivityTaskManager: Displayed ${component}.*?\\+(?:(\\d+)s)?(\\d+)ms`,
  );
  const result: AndroidSystemLog = {};
  for (const line of lines) {
    const at = logcatTime(line);
    if (at === undefined) continue;
    if (result.startAt === undefined) {
      if (start.test(line)) result.startAt = at;
      continue;
    }
    if (result.processStart === undefined && proc.test(line))
      result.processStart = at - result.startAt;
    if (result.displayed === undefined) {
      const match = displayed.exec(line);
      if (match) result.displayed = Number(match[1] ?? 0) * 1000 + Number(match[2]);
    }
  }
  return result;
}

export function parseAmStart(output: string): { totalTime?: number; launchState?: string } {
  const total = /TotalTime: (\d+)/.exec(output);
  const state = /LaunchState: (\w+)/.exec(output);
  return {
    ...(total ? { totalTime: Number(total[1]) } : {}),
    ...(state?.[1] ? { launchState: state[1] } : {}),
  };
}

/**
 * Places the app's marks on the launch command's clock. The `js-entry` mark's
 * log line is where the runtime's clock meets the device's; without it no
 * mark can be placed.
 */
export function gatesFromLog(lines: string[], appId: string, amStart: string): LaunchGates {
  const system = parseAndroidSystemLog(lines, appId);
  const bench = parseBenchLines(lines);
  const gates: LaunchGates = {};
  if (system.processStart !== undefined) gates.processStart = system.processStart;
  if (system.displayed !== undefined) gates.displayed = system.displayed;
  const start = parseAmStart(amStart);
  if (start.totalTime !== undefined) gates.totalTime = start.totalTime;

  const jsEntryLine = markLineTime(lines, 'js-entry');
  const jsEntry = bench.marks['js-entry'];
  if (system.startAt === undefined || jsEntryLine === undefined || jsEntry === undefined)
    return gates;
  const origin = jsEntryLine - system.startAt - jsEntry;
  for (const [name, gate] of Object.entries(MARK_GATES)) {
    const at = bench.marks[name];
    if (at !== undefined) gates[gate] = round(origin + at);
  }
  const first = bench.images[0];
  const settled = imagesSettledAt(bench.images);
  if (first !== undefined) gates.firstImage = round(origin + first);
  if (settled !== undefined) gates.painted = round(origin + settled);
  return gates;
}

function round(value: number): number {
  return Math.round(value * 10) / 10;
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}
