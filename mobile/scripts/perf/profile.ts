/**
 * Turns an ART profile dump into a baseline profile.
 *
 * `adb shell cmd package dump-profiles` writes what the runtime learned about
 * an app in a readable form: which methods ran during startup, which ran often
 * enough to be worth compiling, and which classes were loaded. A baseline
 * profile says the same things in the form the Android Gradle plugin reads,
 * where types are JVM descriptors and each method carries its flags.
 *
 *   void a.B.c(int)          in "startup methods"  ->  SLa/B;->c(I)V
 *   a.B                      in "classes"          ->  La/B;
 */

/** Which list of the dump an entry came from, in the order flags are written. */
const SECTIONS: Record<string, string> = {
  'hot methods': 'H',
  'startup methods': 'S',
  'post startup methods': 'P',
  classes: '',
  // Classes the runtime loaded but does not want preloaded: not ours to ask for.
  'classes-no-preload': '',
};
const FLAG_ORDER = ['H', 'S', 'P'];

const PRIMITIVES: Record<string, string> = {
  void: 'V',
  boolean: 'Z',
  byte: 'B',
  char: 'C',
  short: 'S',
  int: 'I',
  long: 'J',
  float: 'F',
  double: 'D',
};

/** `java.lang.String[]` as `[Ljava/lang/String;`, `int` as `I`. */
export function descriptorOf(type: string): string {
  let base = type.trim();
  let arrays = 0;
  while (base.endsWith('[]')) {
    arrays += 1;
    base = base.slice(0, -2);
  }
  const primitive = PRIMITIVES[base];
  return '['.repeat(arrays) + (primitive ?? `L${base.replace(/\./g, '/')};`);
}

/** `void a.B.c(int, a.B)` as `La/B;->c(ILa/B;)V`, or null if it is not a method. */
export function methodDescriptor(entry: string): string | null {
  const open = entry.indexOf('(');
  const close = entry.lastIndexOf(')');
  if (open === -1 || close < open) return null;
  const head = entry.slice(0, open);
  const space = head.lastIndexOf(' ');
  if (space === -1) return null;
  const qualified = head.slice(space + 1);
  const dot = qualified.lastIndexOf('.');
  if (dot === -1) return null;
  const args = entry.slice(open + 1, close).trim();
  const parameters = args === '' ? [] : args.split(',');
  return (
    `L${qualified.slice(0, dot).replace(/\./g, '/')};->${qualified.slice(dot + 1)}` +
    `(${parameters.map(descriptorOf).join('')})${descriptorOf(head.slice(0, space))}`
  );
}

/** Everything after the entry itself: a trailing comma, and a hot method's inline caches. */
function clean(line: string): string {
  let entry = line.trim().replace(/,$/, '').trim();
  if (entry.endsWith('[]')) entry = entry.slice(0, -2).trim();
  return entry;
}

/**
 * The profile's lines, sorted, each a class or a method with its flags. A
 * method listed in several sections is written once with all of them.
 */
export function toBaselineProfile(dump: string): string[] {
  const flags = new Map<string, Set<string>>();
  const classes = new Set<string>();
  let section: string | null = null;
  for (const line of dump.split('\n')) {
    if (!line.startsWith('\t')) {
      // A new dex file, or the end of the profile: no section is open.
      section = null;
      continue;
    }
    const header = /^\t([\w\- ]+):\s*$/.exec(line);
    if (header) {
      const name = header[1] as string;
      section = name in SECTIONS ? name : null;
      continue;
    }
    if (section === null) continue;
    const entry = clean(line);
    if (entry === '') continue;
    if (SECTIONS[section] === '') {
      classes.add(`L${entry.replace(/\./g, '/')};`);
      continue;
    }
    const descriptor = methodDescriptor(entry);
    if (descriptor === null) continue;
    const seen = flags.get(descriptor) ?? new Set<string>();
    seen.add(SECTIONS[section] as string);
    flags.set(descriptor, seen);
  }
  const lines = [...classes];
  for (const [descriptor, seen] of flags) {
    lines.push(FLAG_ORDER.filter((flag) => seen.has(flag)).join('') + descriptor);
  }
  return lines.sort();
}
