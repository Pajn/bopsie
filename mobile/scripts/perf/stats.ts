/** Medians with their interquartile range, and the comparison of two samples. */

export interface Summary {
  median: number;
  iqr: number;
  n: number;
}

export function quantile(sorted: number[], q: number): number {
  if (sorted.length === 0) return Number.NaN;
  const position = (sorted.length - 1) * q;
  const lower = Math.floor(position);
  const upper = Math.ceil(position);
  const weight = position - lower;
  return (sorted[lower] as number) * (1 - weight) + (sorted[upper] as number) * weight;
}

export function summarize(values: number[]): Summary | undefined {
  const sorted = values.filter((v) => Number.isFinite(v)).sort((a, b) => a - b);
  if (sorted.length === 0) return undefined;
  return {
    median: round(quantile(sorted, 0.5)),
    iqr: round(quantile(sorted, 0.75) - quantile(sorted, 0.25)),
    n: sorted.length,
  };
}

export type Verdict = 'better' | 'worse' | 'same';

/**
 * A change counts when the medians differ by more than both a floor and the
 * average spread of the two samples, so noise does not read as a result.
 */
export function compare(current: Summary, baseline: Summary, floorMs = 20): Verdict {
  const delta = current.median - baseline.median;
  const noise = Math.max(floorMs, (current.iqr + baseline.iqr) / 2);
  if (Math.abs(delta) <= noise) return 'same';
  return delta < 0 ? 'better' : 'worse';
}

export function formatSummary(summary: Summary | undefined): string {
  if (!summary) return '-';
  return `${summary.median} (±${summary.iqr})`;
}

function round(value: number): number {
  return Math.round(value * 10) / 10;
}
