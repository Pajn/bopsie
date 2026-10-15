/**
 * Startup milestones for the performance bench in scripts/perf.
 *
 * Each mark is one log line, `[bench] mark <name> <ms>`, with ms since the
 * JavaScript runtime started. The bench reads them from the device log and
 * places them on the launch command's clock. A handful of lines per launch
 * costs nothing worth measuring, so they are always on. Repeating marks, such as images
 * arriving, stop once startup is over, so scrolling a list later logs nothing.
 */

const origin = globalThis.performance?.now() ?? 0;
const seen = new Set<string>();

/** Records `name` once per launch; later calls with the same name are ignored. */
export function mark(name: string): void {
  if (seen.has(name)) return;
  seen.add(name);
  const at = (globalThis.performance?.now() ?? 0) - origin;
  console.log(`[bench] mark ${name} ${at.toFixed(1)}`);
}

/** Repeating marks after this long, in ms, are no longer startup's. */
const STARTUP_MS = 15_000;

/** Records `name` on every call during startup, for repeating events such as images arriving. */
export function markEach(name: string): void {
  const at = (globalThis.performance?.now() ?? 0) - origin;
  if (at > STARTUP_MS) return;
  console.log(`[bench] mark ${name} ${at.toFixed(1)}`);
}
