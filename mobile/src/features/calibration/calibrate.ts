import type { MaConnection } from '@bopsie/client/api/client';
import { startCalibration } from '@bopsie/client/api/endpoints/admin';
import type { LocalPlayer } from '@/player/Player';
import { hearChirps, type SpeakerResult, speakerResults } from './detect';

/** How long past the last chirp the phone keeps listening, for its echo and for the network. */
const TAIL_MS = 700;
/** The longest a calibration is listened to, whatever the server says. */
const MAX_LISTEN_MS = 120_000;
/** Speakers this close count as in step. */
const IN_STEP_MS = 2;

export interface Heard {
  results: SpeakerResult[];
  /** Speakers the group carries in their own multiroom, which were not measured. */
  skipped: string[];
}

/**
 * Plays a calibration on `playerId`'s group and listens to it. `onListening` is told how long
 * listening takes, once it starts. Throws with a message for the user.
 */
export async function listen(
  connection: MaConnection,
  player: LocalPlayer,
  playerId: string,
  onListening: (ms: number) => void,
): Promise<Heard> {
  const calibration = await startCalibration(connection, playerId);
  const now = await player.serverNowUs();
  if (now === null) {
    throw new Error('This phone needs to be connected as a player, to keep time with the server.');
  }
  const ms = Math.min((calibration.ends_at_us - now) / 1000 + TAIL_MS, MAX_LISTEN_MS);
  // The server's clock starts again when it restarts; the phone's player follows a moment later.
  if (ms <= TAIL_MS) {
    throw new Error(
      "This phone hasn't caught up with the server's clock yet, as after the server restarted. Try again in a moment.",
    );
  }
  onListening(ms);
  const recording = await player.recordCalibration(ms);
  if (recording === null) throw new Error('This phone cannot record.');
  // Lets the screen say it is working before the search takes the thread.
  await new Promise((resolve) => setTimeout(resolve, 50));
  return {
    results: speakerResults(hearChirps(recording, calibration)),
    skipped: calibration.skipped,
  };
}

/** A speaker's line in the results. */
export interface ResultLine {
  endpointId: string;
  name: string;
  text: string;
  ok: boolean;
}

/** What to tell the user of each speaker: how far ahead of the latest it plays, or why it
 * cannot be told. The latest is the one the others are made to wait for. */
export function describeResults(results: SpeakerResult[]): ResultLine[] {
  const delays = results.flatMap((r) => (r.delayMs === null ? [] : [r.delayMs]));
  const latest = delays.length > 0 ? Math.max(...delays) : 0;
  return results.map((result) => {
    const line = { endpointId: result.endpointId, name: result.name };
    if (result.problem === 'not-heard') {
      return { ...line, ok: false, text: 'Not heard. Turn it up, or move closer to it.' };
    }
    if (result.problem === 'unsteady' || result.delayMs === null) {
      return {
        ...line,
        ok: false,
        text: `Heard unevenly, ${Math.round(result.spreadMs)} ms apart. Try again somewhere quieter.`,
      };
    }
    const ahead = latest - result.delayMs;
    if (ahead < IN_STEP_MS) return { ...line, ok: true, text: 'In step with the latest.' };
    return { ...line, ok: true, text: `${Math.round(ahead)} ms ahead; it will wait.` };
  });
}

/** Each speaker that was measured, and how late it was heard, for the server to set from. */
export function delaysOf(results: SpeakerResult[]): Record<string, number> {
  return Object.fromEntries(
    results.flatMap((r) => (r.delayMs === null ? [] : [[r.endpointId, r.delayMs]])),
  );
}
