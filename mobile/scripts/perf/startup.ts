#!/usr/bin/env node
/**
 * Cold-start bench for Android devices and emulators.
 *
 *   node scripts/perf/startup.ts --apk <path> --out <file.json> [--runs 10] [--device <serial>]
 *   node scripts/perf/startup.ts --compare <baseline.json> <current.json>
 *
 * Each run force-stops the app, clears the log, starts the main activity with
 * `am start -W`, samples the process's CPU per thread on the device, waits for
 * the first screen's images to settle, and reads the app's `[bench] mark`
 * lines and the activity manager's milestones back from logcat. One warm-up
 * launch after the install is discarded. The app must already be signed in
 * on the device; the bench does not touch its state.
 */

import { type ChildProcess, execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, openSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { parseArgs } from 'node:util';
import { adb, describeDevice, ensureUnlocked, installApk, sleep } from './device.ts';
import {
  GATE_ORDER,
  gatesFromLog,
  type LaunchGates,
  parseAmStart,
  parseBenchLines,
} from './parse.ts';
import { compare, formatSummary, type Summary, summarize } from './stats.ts';

const APP_ID = 'io.github.pajn.bopsie';
const SAMPLER_SOURCE = path.join(import.meta.dirname, 'sampler-android.sh');
const SAMPLER_ON_DEVICE = '/data/local/tmp/bopsie-bench-sampler.sh';
const SETTLE_TIMEOUT_MS = 20_000;
const IMAGE_QUIET_MS = 1500;
const SAMPLE_INTERVAL_S = 0.05;

export interface CpuSummary {
  /** Percent of one core over the launch, per thread group. */
  main: number;
  js: number;
  render: number;
  total: number;
  /** Seconds sampled. */
  seconds: number;
}

export interface LaunchRun {
  index: number;
  gates: LaunchGates;
  cpu?: CpuSummary;
  warnings: string[];
}

export interface StartupResult {
  version: 1;
  device: string;
  apk: string;
  commit: string;
  measuredAt: string;
  runs: LaunchRun[];
}

/** Cumulative CPU nanoseconds per group, from the sampler's lines. */
function parseSampler(
  output: string,
): Array<{ at: number; main: number; js: number; render: number; total: number }> {
  const samples = [];
  for (const line of output.split('\n')) {
    const match = /^(\d+(?:\.\d+)?) main (\d+) js (\d+) render (\d+) total (\d+)/.exec(line);
    if (!match) continue;
    samples.push({
      at: Number(match[1]),
      main: Number(match[2]),
      js: Number(match[3]),
      render: Number(match[4]),
      total: Number(match[5]),
    });
  }
  return samples;
}

/** Average percent of one core per group between the first sample and `untilAt` (epoch seconds). */
function cpuSummary(output: string, untilAt: number | undefined): CpuSummary | undefined {
  const samples = parseSampler(output);
  const first = samples[0];
  if (!first) return undefined;
  const window = untilAt === undefined ? samples : samples.filter((s) => s.at <= untilAt);
  const last = window[window.length - 1] ?? samples[samples.length - 1];
  if (!last || last.at <= first.at) return undefined;
  const seconds = last.at - first.at;
  const pct = (ns: number) => Math.round((ns / 1e9 / seconds) * 1000) / 10;
  return {
    main: pct(last.main - first.main),
    js: pct(last.js - first.js),
    render: pct(last.render - first.render),
    total: pct(last.total - first.total),
    seconds: Math.round(seconds * 100) / 100,
  };
}

async function coldLaunch(
  serial: string | undefined,
  index: number,
  logDir: string,
): Promise<LaunchRun> {
  const base = path.join(logDir, `run_${index}`);
  const warnings: string[] = [];
  adb(serial, ['shell', `am force-stop ${APP_ID}`], true);
  await sleep(2000);
  adb(serial, ['logcat', '-c']);

  const cpuLog = `${base}.cpu`;
  writeFileSync(cpuLog, '');
  const fd = openSync(cpuLog, 'a');
  const sampler: ChildProcess = spawn(
    'adb',
    [
      ...(serial ? ['-s', serial] : []),
      'shell',
      `sh ${SAMPLER_ON_DEVICE} ${APP_ID} ${SAMPLE_INTERVAL_S} ${Math.ceil((SETTLE_TIMEOUT_MS / 1000 + 5) / SAMPLE_INTERVAL_S)}`,
    ],
    { stdio: ['ignore', fd, fd] },
  );
  await sleep(300);

  const amStart = adb(serial, ['shell', `am start -W -n ${APP_ID}/.MainActivity`]);
  writeFileSync(`${base}.start`, amStart);

  // Wait for the first screen's images to stop arriving, or for a quiet
  // period after the home marks when the screen has no images to show.
  const deadline = Date.now() + SETTLE_TIMEOUT_MS;
  let lastChange = Date.now();
  let seenImages = 0;
  let seenHome = false;
  while (Date.now() < deadline) {
    const lines = adb(serial, ['logcat', '-d', '-v', 'epoch', '-e', 'bench\\] mark'], true).split(
      '\n',
    );
    const log = parseBenchLines(lines);
    if (log.images.length !== seenImages) {
      seenImages = log.images.length;
      lastChange = Date.now();
    }
    if (!seenHome && log.marks['home-render'] !== undefined) {
      seenHome = true;
      lastChange = Date.now();
    }
    if (seenHome && Date.now() - lastChange > IMAGE_QUIET_MS) break;
    await sleep(250);
  }
  const settledAt = Date.now() / 1000;
  if (!seenHome) warnings.push(`home screen not reached within ${SETTLE_TIMEOUT_MS} ms`);

  const log = adb(serial, ['logcat', '-d', '-v', 'threadtime', '-v', 'epoch', '-v', 'usec']);
  writeFileSync(`${base}.log`, log);
  adb(serial, ['shell', `pkill -f ${SAMPLER_ON_DEVICE}`], true);
  sampler.kill('SIGTERM');

  const gates = gatesFromLog(log.split('\n'), APP_ID, amStart);
  const start = parseAmStart(amStart);
  if (start.launchState && start.launchState !== 'COLD') {
    warnings.push(`launch state was ${start.launchState}, not COLD`);
  }
  for (const gate of ['jsEntry', 'firstScreenCommit', 'homeRender'] as const) {
    if (gates[gate] === undefined) warnings.push(`missing gate ${gate}`);
  }
  const cpu = cpuSummary(readFileSync(cpuLog, 'utf8'), settledAt);
  return { index, gates, ...(cpu ? { cpu } : {}), warnings };
}

async function bench(argv: string[]) {
  const { values } = parseArgs({
    args: argv,
    options: {
      apk: { type: 'string' },
      out: { type: 'string' },
      runs: { type: 'string', default: '10' },
      device: { type: 'string' },
      'skip-install': { type: 'boolean', default: false },
      /** ART compile mode applied after install: speed (default), speed-profile, verify. */
      compile: { type: 'string', default: 'speed' },
    },
  });
  if (!values.out || (!values.apk && !values['skip-install'])) {
    console.error('usage: startup.ts --apk <path> --out <file.json> [--runs N] [--device serial]');
    process.exit(2);
  }
  const serial = values.device;
  const runs = Number(values.runs);
  const logDir = path.join(path.dirname(values.out), 'logs', path.basename(values.out, '.json'));
  mkdirSync(logDir, { recursive: true });

  if (!values['skip-install']) {
    const apk = values.apk as string;
    if (!existsSync(apk)) throw new Error(`APK not found: ${apk}`);
    installApk(serial, apk);
  }
  // Android compiles a freshly installed app in the background over its first
  // launches, so launches get faster on their own for a while. Compiling it
  // fully up front puts every build on the same footing.
  adb(serial, ['shell', `cmd package compile -m ${values.compile} -f ${APP_ID}`], true);
  adb(serial, ['push', SAMPLER_SOURCE, SAMPLER_ON_DEVICE]);
  adb(serial, ['shell', `chmod 755 ${SAMPLER_ON_DEVICE}`]);
  ensureUnlocked(serial);

  const result: StartupResult = {
    version: 1,
    device: describeDevice(serial),
    apk: `${values.apk ?? '(installed)'} compiled ${values.compile}`,
    commit: gitHead(),
    measuredAt: new Date().toISOString(),
    runs: [],
  };
  console.log(`${result.device}: ${runs} cold launches`);
  console.log('warm-up launch');
  await coldLaunch(serial, 0, logDir);
  for (let i = 1; i <= runs; i++) {
    const run = await coldLaunch(serial, i, logDir);
    result.runs.push(run);
    const brief = GATE_ORDER.filter((g) => run.gates[g] !== undefined)
      .map((g) => `${g} ${run.gates[g]}`)
      .join(', ');
    console.log(`run ${i}: ${brief}${run.warnings.length ? ` [${run.warnings.join('; ')}]` : ''}`);
  }
  adb(serial, ['shell', `am force-stop ${APP_ID}`], true);
  writeFileSync(values.out, JSON.stringify(result, null, 2));
  console.log(`\n${renderTable(result)}`);
  console.log(`written ${values.out}`);
}

export function summaries(result: StartupResult): Record<string, Summary | undefined> {
  const out: Record<string, Summary | undefined> = {};
  for (const gate of GATE_ORDER) {
    out[gate] = summarize(
      result.runs.map((r) => r.gates[gate]).filter((v): v is number => v !== undefined),
    );
  }
  // Spans from Home's render, which the device's own launch drift does not move.
  const span = (to: keyof LaunchGates) =>
    summarize(
      result.runs
        .map((r) =>
          r.gates[to] !== undefined && r.gates.homeRender !== undefined
            ? r.gates[to] - r.gates.homeRender
            : undefined,
        )
        .filter((v): v is number => v !== undefined),
    );
  out['home→shelves'] = span('homeShelves');
  out['home→mounted'] = span('homeMounted');
  out['home→painted'] = span('painted');
  for (const group of ['main', 'js', 'render', 'total'] as const) {
    out[`cpu.${group}`] = summarize(
      result.runs.map((r) => r.cpu?.[group]).filter((v): v is number => v !== undefined),
    );
  }
  return out;
}

export function renderTable(result: StartupResult, baseline?: StartupResult): string {
  const current = summaries(result);
  const base = baseline ? summaries(baseline) : undefined;
  const rows = [
    `| Gate | ${baseline ? 'Baseline | Current | Change' : 'Median (±IQR) ms'} |`,
    `|---|${baseline ? '---|---|---' : '---'}|`,
  ];
  for (const key of Object.keys(current)) {
    const summary = current[key];
    if (!summary) continue;
    if (!base) {
      rows.push(`| ${key} | ${formatSummary(summary)} |`);
      continue;
    }
    const before = base[key];
    const verdict = before ? compare(summary, before) : 'same';
    const delta = before
      ? `${summary.median - before.median > 0 ? '+' : ''}${Math.round((summary.median - before.median) * 10) / 10}`
      : '';
    const label = verdict === 'same' ? '' : verdict === 'better' ? ' better' : ' worse';
    rows.push(
      `| ${key} | ${formatSummary(before)} | ${formatSummary(summary)} | ${delta}${label} |`,
    );
  }
  const warnings = result.runs.flatMap((r) => r.warnings.map((w) => `run ${r.index}: ${w}`));
  return `${result.device}, ${result.runs.length} launches, ${result.commit}\n\n${rows.join('\n')}${warnings.length ? `\n\nWarnings:\n${warnings.map((w) => `- ${w}`).join('\n')}` : ''}`;
}

function gitHead(): string {
  try {
    return execFileSync('git', ['rev-parse', '--short', 'HEAD'], { encoding: 'utf8' }).trim();
  } catch {
    return 'unknown';
  }
}

/** `pnpm run <script> -- --flag` passes the separator through to the script. */
function args(argv: string[]): string[] {
  return argv[0] === '--' ? argv.slice(1) : argv;
}

async function main(argv: string[]) {
  if (argv[0] === '--compare') {
    const [, baselinePath, currentPath] = argv;
    if (!baselinePath || !currentPath) {
      console.error('usage: startup.ts --compare <baseline.json> <current.json>');
      process.exit(2);
    }
    const baseline = JSON.parse(readFileSync(baselinePath, 'utf8')) as StartupResult;
    const current = JSON.parse(readFileSync(currentPath, 'utf8')) as StartupResult;
    console.log(renderTable(current, baseline));
    return;
  }
  await bench(argv);
}

main(args(process.argv.slice(2))).catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
});
