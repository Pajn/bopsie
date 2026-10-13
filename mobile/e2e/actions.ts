/**
 * Playwright-style actions for Detox.
 *
 * Detox synchronization is off in this project (see `.detoxrc.js`), so
 * nothing waits for the app to go idle between steps. These helpers put the
 * waiting back where it belongs: every action waits for its target to be
 * ready and still before acting, and every assertion polls until it holds or
 * the timeout passes. Tests read as a list of intentions and stay reliable.
 */
import { by, expect as detoxExpect, device, element, waitFor } from 'detox';

/** Default time an action or assertion gets before giving up. */
export const ACTION_TIMEOUT_MS = 10_000;
/** How long an element gets to stop moving before it is acted on anyway. */
const SETTLE_TIMEOUT_MS = 2_000;
/**
 * Gap between frame readings; two alike readings count as settled. A screen
 * sliding in eases out, so its last frames barely differ: the gap has to be
 * long enough that alike readings mean the transition is over, because the
 * platform drops taps delivered while one is still running.
 */
const SETTLE_POLL_MS = 300;
/** How long an element that never reads as visible gets to exist instead. */
const EXISTENCE_TIMEOUT_MS = 1_000;
/** Gap between tap attempts while the hit test keeps turning one down. */
const TAP_RETRY_MS = 250;
/** How long one look at the screen lasts, and how long the device then gets to itself. */
const LOOK_MS = 250;

/**
 * How Detox words a hit test it has turned down, as opposed to an element it
 * could not find at all. Only these are worth another go.
 */
const NOT_HITTABLE = /not hittable|is not visible/i;

type Target = Detox.NativeElement;
type Direction = 'up' | 'down' | 'left' | 'right';

export interface ActionOptions {
  timeout?: number;
}

export interface TapOptions extends ActionOptions {
  point?: Detox.Point2D;
}

export interface TypeOptions extends ActionOptions {
  /** Clear any existing text first. Default: false. */
  clear?: boolean;
  /** Tap the return key afterwards. Default: false. */
  submit?: boolean;
}

export interface ScrollOptions extends ActionOptions {
  /** Distance per scroll step. Default: 200. */
  step?: number;
  /** Where in the scroll view the gesture starts, 0 to 1. Default: centre. */
  startX?: number;
  startY?: number;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

// Selectors -----------------------------------------------------------------

/** Element by `testID`. */
/**
 * Android renders bottom-sheet content twice, both copies live, so a matcher
 * that is unique on iOS finds two views there. The first is as good as any.
 */
export const first = (target: Detox.IndexableNativeElement): Target =>
  device.getPlatform() === 'android' ? target.atIndex(0) : target;

export const byId = (id: string): Target => first(element(by.id(id)));
/** Element by exact visible text. */
export const byText = (text: string): Target => first(element(by.text(text)));
/** Element by accessibility label. */
export const byLabel = (label: string): Target => first(element(by.label(label)));

// Readiness -----------------------------------------------------------------

/** `target`'s frame as a comparable string, or '' where it has none. */
async function frameOf(target: Target): Promise<string> {
  const attributes = (await target.getAttributes()) as
    | Detox.IosElementAttributes
    | Detox.AndroidElementAttributes
    | { elements: Array<Detox.IosElementAttributes | Detox.AndroidElementAttributes> };
  const first = 'elements' in attributes ? attributes.elements[0] : attributes;
  const frame = first?.frame;
  return frame ? `${frame.x},${frame.y},${frame.width},${frame.height}` : '';
}

/**
 * Waits until `target` has stopped moving.
 *
 * A tap that lands while the screen is still settling is delivered to
 * wherever the element was, and the app reacts to none of it: the tap
 * reports success and nothing happens. Detox's visibility check says an
 * element can be seen, not that it has come to rest, and with
 * synchronization on the idle wait covered the difference.
 *
 * Two alike readings of the frame mean it has settled. An element that never
 * settles is acted on anyway once the deadline passes, so a permanently
 * animating screen fails on the action rather than here.
 */
async function waitUntilStill(target: Target): Promise<void> {
  const deadline = Date.now() + SETTLE_TIMEOUT_MS;
  let previous: string | null = null;
  for (;;) {
    let current: string;
    try {
      current = await frameOf(target);
    } catch {
      // Unreadable attributes are the action's problem to report, not ours.
      return;
    }
    if (current !== '' && current === previous) return;
    previous = current;
    if (Date.now() >= deadline) return;
    await sleep(SETTLE_POLL_MS);
  }
}

/**
 * Waits for `target` to be ready to act on.
 *
 * Visible is the condition worth waiting for, but Detox reads visibility
 * from the pixels an element covers and some containers defeat that, such
 * as masked or faded views whose children tap perfectly well. So an element
 * that will not read as visible but does exist is acted on anyway, and the
 * action's own hit test has the final say. An element that is neither
 * reports the visibility failure, which says more than a missing one would.
 */
async function waitUntilActionable(target: Target, timeout: number): Promise<void> {
  try {
    await waitFor(target).toBeVisible().withTimeout(timeout);
  } catch (notVisible) {
    try {
      await waitFor(target).toExist().withTimeout(EXISTENCE_TIMEOUT_MS);
    } catch {
      throw notVisible;
    }
  }
}

/**
 * Runs `attempt` against a ready, still `target`, retrying while Detox's hit
 * test is the only thing in the way.
 *
 * Detox decides whether a gesture may land by comparing the pixels an element
 * covers before and after drawing it. An element fading or blurring into
 * place covers its own pixels and the hit test turns the gesture down. The
 * element is there and the same gesture lands a moment later, so the wait
 * belongs around the gesture rather than only before it.
 *
 * Detox raises this before dispatching anything, so a turned-down gesture has
 * not reached the app and trying again cannot double it. Any other failure
 * is reported as it comes.
 */
async function act(target: Target, timeout: number, attempt: () => Promise<void>): Promise<void> {
  const deadline = Date.now() + timeout;
  await waitUntilActionable(target, timeout);
  for (;;) {
    await waitUntilStill(target);
    try {
      await attempt();
      return;
    } catch (rejected) {
      if (Date.now() >= deadline || !NOT_HITTABLE.test(String(rejected))) {
        throw rejected;
      }
      await sleep(TAP_RETRY_MS);
    }
  }
}

// Actions -------------------------------------------------------------------

/** Taps `target` once it is there and has stopped moving. */
export async function tap(
  target: Target,
  { timeout = ACTION_TIMEOUT_MS, point }: TapOptions = {},
): Promise<void> {
  await act(target, timeout, () => target.tap(point));
}

/** Long-presses `target` once it is there and has stopped moving. */
export async function longPress(
  target: Target,
  { timeout = ACTION_TIMEOUT_MS, point }: TapOptions = {},
): Promise<void> {
  await act(target, timeout, () => (point ? target.longPress(point) : target.longPress()));
}

/**
 * Types into `target`. Taps it first so the field is focused on both
 * platforms, and optionally clears it and submits afterwards.
 */
export async function typeText(
  target: Target,
  text: string,
  { timeout = ACTION_TIMEOUT_MS, clear = false, submit = false }: TypeOptions = {},
): Promise<void> {
  await tap(target, { timeout });
  if (clear) await target.clearText();
  await target.typeText(submit ? `${text}\n` : text);
}

/** Replaces the contents of `target` with `text` without keystrokes. */
export async function fill(
  target: Target,
  text: string,
  { timeout = ACTION_TIMEOUT_MS }: ActionOptions = {},
): Promise<void> {
  await act(target, timeout, () => target.replaceText(text));
}

/** Swipes on `target` in `direction`. */
export async function swipe(
  target: Target,
  direction: Direction,
  { timeout = ACTION_TIMEOUT_MS }: ActionOptions = {},
  speed: Detox.Speed = 'fast',
): Promise<void> {
  await act(target, timeout, () => target.swipe(direction, speed));
}

/** Gap between scroll searches that ran out of content before the target appeared. */
const SCROLL_RETRY_MS = 500;

/**
 * Scrolls `scrollView` in `direction` until `target` is visible.
 *
 * Detox ends a search when the scroll view runs out of content. Lists lay
 * out lazily, so the end can be reached before the target has been measured
 * into view; the search is repeated until the timeout passes.
 */
export async function scrollTo(
  target: Target,
  scrollView: Detox.NativeMatcher,
  direction: Direction,
  {
    timeout = ACTION_TIMEOUT_MS,
    step = 200,
    startX = Number.NaN,
    startY = Number.NaN,
  }: ScrollOptions = {},
): Promise<void> {
  const deadline = Date.now() + timeout;
  for (;;) {
    try {
      await waitFor(target)
        .toBeVisible()
        .whileElement(scrollView)
        .scroll(step, direction, startX, startY);
      return;
    } catch (error) {
      if (Date.now() >= deadline) throw error;
      await sleep(SCROLL_RETRY_MS);
    }
  }
}

/** Scrolls `scrollView` to its very start or end. */
export async function scrollToEdge(
  scrollView: Target,
  edge: 'top' | 'bottom' | 'left' | 'right',
  { timeout = ACTION_TIMEOUT_MS }: ActionOptions = {},
): Promise<void> {
  await act(scrollView, timeout, () => scrollView.scrollTo(edge));
}

// Assertions ----------------------------------------------------------------

/**
 * Waits for `look` to pass, in short looks with the device left alone between
 * them.
 *
 * Detox looks at the screen on the same thread the device draws and animates
 * on, and a wait of its own looks again the moment it has finished looking.
 * Anything still on its way there — a sheet climbing out of its dock — is
 * then never given the frames it needs to arrive, so waiting for it is what
 * stops it coming: watched, it stays where it was until the wait gives up.
 * The pauses are what let it finish.
 */
async function inLooks(
  look: (ms: number) => Promise<unknown>,
  timeout: number,
  what: string,
): Promise<void> {
  const deadline = Date.now() + timeout;
  for (;;) {
    try {
      await look(LOOK_MS);
      return;
    } catch (error) {
      if (Date.now() < deadline) {
        await sleep(LOOK_MS);
        continue;
      }
      // Each look is short, and its own message would report that rather
      // than the time the assertion was actually given.
      throw new Error(`${what} within ${timeout / 1000}s. ${(error as Error).message}`);
    }
  }
}

/** Passes once `target` is visible. */
export async function expectVisible(
  target: Target,
  { timeout = ACTION_TIMEOUT_MS }: ActionOptions = {},
): Promise<void> {
  await inLooks((ms) => waitFor(target).toBeVisible().withTimeout(ms), timeout, 'Never visible');
}

/** Passes once `target` is gone or hidden. */
export async function expectNotVisible(
  target: Target,
  { timeout = ACTION_TIMEOUT_MS }: ActionOptions = {},
): Promise<void> {
  await inLooks(
    (ms) => waitFor(target).not.toBeVisible().withTimeout(ms),
    timeout,
    'Never went away',
  );
}

/** Passes once `target` exists in the hierarchy, visible or not. */
export async function expectExists(
  target: Target,
  { timeout = ACTION_TIMEOUT_MS }: ActionOptions = {},
): Promise<void> {
  await waitFor(target).toExist().withTimeout(timeout);
}

/** Passes once `target` shows exactly `text`. */
export async function expectText(
  target: Target,
  text: string,
  { timeout = ACTION_TIMEOUT_MS }: ActionOptions = {},
): Promise<void> {
  await waitFor(target).toHaveText(text).withTimeout(timeout);
}

/** Passes once `target`'s value (a slider, switch or input) equals `value`. */
export async function expectValue(
  target: Target,
  value: string,
  { timeout = ACTION_TIMEOUT_MS }: ActionOptions = {},
): Promise<void> {
  await waitFor(target).toHaveValue(value).withTimeout(timeout);
}

/** Passes once `target` is focused. */
export async function expectFocused(
  target: Target,
  { timeout = ACTION_TIMEOUT_MS }: ActionOptions = {},
): Promise<void> {
  await waitFor(target).toBeFocused().withTimeout(timeout);
}

/** Immediate, non-polling Detox `expect`, for the rare case it is wanted. */
export const expectNow = detoxExpect;

/**
 * Waits for a bottom sheet to finish presenting. Sheets animate natively, so
 * their children look stationary to frame polling while they are still moving.
 */
export async function waitForSheet(timeout = ACTION_TIMEOUT_MS): Promise<void> {
  await waitFor(element(by.id('sheet-presented')).atIndex(0))
    .toExist()
    .withTimeout(timeout);
}
