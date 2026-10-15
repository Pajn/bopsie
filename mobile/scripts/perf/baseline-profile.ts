#!/usr/bin/env node
/**
 * Records a baseline profile from cold launches on an Android device.
 *
 *   node scripts/perf/baseline-profile.ts --apk <path> [--runs 5] [--device <serial>]
 *
 * A baseline profile tells Android which classes and methods a launch needs,
 * so the runtime compiles and lays them out ahead of the first launch instead
 * of learning it over several. The runtime already works this out for itself
 * while an app runs; this reads that back out.
 *
 * The app is installed, set to be interpreted so the runtime records what it
 * runs, launched cold a few times, and then asked for what it learned. Only
 * launching happens in between, so what comes back describes a launch and not
 * a browsing session. The result is written where the config plugin picks it
 * up, and belongs in the repository.
 *
 * Each launch has to sit in the foreground longer than the runtime's write
 * timer, so recording takes a few minutes. The app must already be signed in
 * on the device, and the device unlocked.
 */

import { existsSync, writeFileSync } from 'node:fs';
import { parseArgs } from 'node:util';
import { adb, apiLevel, describeDevice, ensureUnlocked, installApk, sleep } from './device.ts';
import { parseBenchLines } from './parse.ts';
import { toBaselineProfile } from './profile.ts';

const APP_ID = 'io.github.pajn.cratie';
const PROFILE_DIR = '/data/misc/profman';
const DEFAULT_OUT = 'profiles/baseline-prof.txt';

/** How long to wait for the first screen before giving up on a launch. */
const HOME_TIMEOUT_MS = 20_000;
/**
 * Time in the foreground after the first screen.
 *
 * The runtime keeps what it learns in memory and writes it out on a timer,
 * no more often than every forty seconds. An app killed before that has
 * taught the device nothing, so each run has to outlast the timer.
 */
const DWELL_MS = 50_000;
/** Leaving the foreground prompts a write of whatever is still in memory. */
const BACKGROUND_MS = 5_000;
const SAVE_MS = 10_000;

async function coldLaunch(serial: string | undefined, index: number): Promise<void> {
  adb(serial, ['shell', `am force-stop ${APP_ID}`], true);
  await sleep(1000);
  adb(serial, ['logcat', '-c']);
  adb(serial, ['shell', `am start -W -n ${APP_ID}/.MainActivity`]);

  const deadline = Date.now() + HOME_TIMEOUT_MS;
  let reached = false;
  while (Date.now() < deadline) {
    const lines = adb(serial, ['logcat', '-d', '-e', 'bench\\] mark'], true).split('\n');
    if (parseBenchLines(lines).marks['home-render'] !== undefined) {
      reached = true;
      break;
    }
    await sleep(250);
  }
  if (!reached) throw new Error(`run ${index}: the first screen did not arrive`);

  await sleep(DWELL_MS);
  adb(serial, ['shell', 'input keyevent KEYCODE_HOME'], true);
  await sleep(BACKGROUND_MS);
}

/** What the runtime has learned about the app so far, in its own readable form. */
function dumpProfile(serial: string | undefined): string {
  adb(serial, ['shell', `cmd package dump-profiles ${APP_ID}`], true);
  const listing = adb(serial, ['shell', `ls ${PROFILE_DIR}`], true);
  const name = listing
    .split('\n')
    .map((line) => line.trim())
    .find((line) => line.startsWith(APP_ID) && line.endsWith('.txt'));
  if (!name) throw new Error(`No profile dump under ${PROFILE_DIR}; is the build profileable?`);
  return adb(serial, ['shell', `cat ${PROFILE_DIR}/${name}`], true);
}

/**
 * Clears what the runtime has already learned, so the profile describes these
 * launches alone. The command arrived in Android 14; before that, installing
 * a build the device has not seen is what resets it.
 */
function clearProfiles(serial: string | undefined): void {
  if (apiLevel(serial) >= 34) adb(serial, ['shell', `pm art clear-app-profiles ${APP_ID}`], true);
}

/** `pnpm run <script> -- --flag` passes the separator through to the script. */
function args(argv: string[]): string[] {
  return argv[0] === '--' ? argv.slice(1) : argv;
}

async function main(argv: string[]) {
  const { values } = parseArgs({
    args: argv,
    options: {
      apk: { type: 'string' },
      out: { type: 'string', default: DEFAULT_OUT },
      runs: { type: 'string', default: '3' },
      device: { type: 'string' },
      'skip-install': { type: 'boolean', default: false },
    },
  });
  if (!values.apk && !values['skip-install']) {
    console.error('usage: baseline-profile.ts --apk <path> [--runs N] [--device serial]');
    process.exit(2);
  }
  const serial = values.device;
  const runs = Number(values.runs);

  if (!values['skip-install']) {
    const apk = values.apk as string;
    if (!existsSync(apk)) throw new Error(`APK not found: ${apk}`);
    installApk(serial, apk);
  }
  clearProfiles(serial);
  // Compiled code teaches the runtime nothing: it only learns from what it
  // interprets and compiles as it goes.
  adb(serial, ['shell', `cmd package compile -m verify -f ${APP_ID}`], true);
  ensureUnlocked(serial);

  const existing = toBaselineProfile(dumpProfile(serial));
  if (existing.length > 0) {
    throw new Error(
      `The device already holds a profile of ${existing.length} entries for this build, which ` +
        'would describe more than a launch. Record against a build the device has not seen, or ' +
        'use a device on Android 14 or later, where the profile can be cleared.',
    );
  }

  console.log(`${describeDevice(serial)}: ${runs} cold launches`);
  for (let index = 1; index <= runs; index++) {
    await coldLaunch(serial, index);
    console.log(`run ${index} recorded`);
  }
  adb(serial, ['shell', `am force-stop ${APP_ID}`], true);
  await sleep(SAVE_MS);

  const lines = toBaselineProfile(dumpProfile(serial));
  if (lines.length === 0) {
    throw new Error('The runtime wrote no profile. Try more runs, or a longer wait.');
  }
  const methods = lines.filter((line) => line.includes('->')).length;
  writeFileSync(values.out, `${lines.join('\n')}\n`);
  console.log(`\n${lines.length - methods} classes and ${methods} methods -> ${values.out}`);
}

main(args(process.argv.slice(2))).catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
});
