/**
 * An in-place radix-2 FFT over separate real and imaginary arrays, whose length is a power of
 * two. `inverse` transforms back and scales by 1/n.
 */
export function fft(re: Float64Array, im: Float64Array, inverse = false): void {
  const n = re.length;
  if (n !== im.length || (n & (n - 1)) !== 0) {
    throw new Error('An FFT needs two arrays of one power-of-two length');
  }
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      [re[i], re[j]] = [re[j] as number, re[i] as number];
      [im[i], im[j]] = [im[j] as number, im[i] as number];
    }
  }
  for (let size = 2; size <= n; size <<= 1) {
    const angle = ((inverse ? 2 : -2) * Math.PI) / size;
    const stepRe = Math.cos(angle);
    const stepIm = Math.sin(angle);
    const half = size >> 1;
    for (let start = 0; start < n; start += size) {
      let wRe = 1;
      let wIm = 0;
      for (let k = 0; k < half; k++) {
        const a = start + k;
        const b = a + half;
        const bRe = re[b] as number;
        const bIm = im[b] as number;
        const tRe = bRe * wRe - bIm * wIm;
        const tIm = bRe * wIm + bIm * wRe;
        re[b] = (re[a] as number) - tRe;
        im[b] = (im[a] as number) - tIm;
        re[a] = (re[a] as number) + tRe;
        im[a] = (im[a] as number) + tIm;
        const next = wRe * stepRe - wIm * stepIm;
        wIm = wRe * stepIm + wIm * stepRe;
        wRe = next;
      }
    }
  }
  if (inverse) {
    for (let i = 0; i < n; i++) {
      re[i] = (re[i] as number) / n;
      im[i] = (im[i] as number) / n;
    }
  }
}

/** The smallest power of two at least `n`. */
export function powerOfTwo(n: number): number {
  let size = 1;
  while (size < n) size <<= 1;
  return size;
}

/**
 * How well `pattern` matches `signal` at each lag, as the envelope of their cross-correlation:
 * `out[k]` is the magnitude of the analytic signal of the sum of `signal[k + i] * pattern[i]`,
 * for every lag where the pattern fits. The envelope peaks where the pattern lines up, without
 * the ripple of the pattern's own frequencies.
 */
export function correlationEnvelope(signal: Float64Array, pattern: Float64Array): Float64Array {
  const lags = signal.length - pattern.length + 1;
  if (lags <= 0) return new Float64Array(0);
  const n = powerOfTwo(signal.length);
  const aRe = new Float64Array(n);
  const aIm = new Float64Array(n);
  const bRe = new Float64Array(n);
  const bIm = new Float64Array(n);
  aRe.set(signal);
  bRe.set(pattern);
  fft(aRe, aIm);
  fft(bRe, bIm);
  // The signal's spectrum times the pattern's conjugate, its negative frequencies dropped and
  // its positive ones doubled: the analytic signal of the correlation.
  for (let i = 0; i < n; i++) {
    const scale = i === 0 || i === n / 2 ? 1 : i < n / 2 ? 2 : 0;
    const re = (aRe[i] as number) * (bRe[i] as number) + (aIm[i] as number) * (bIm[i] as number);
    const im = (aIm[i] as number) * (bRe[i] as number) - (aRe[i] as number) * (bIm[i] as number);
    aRe[i] = re * scale;
    aIm[i] = im * scale;
  }
  fft(aRe, aIm, true);
  const out = new Float64Array(lags);
  for (let k = 0; k < lags; k++) out[k] = Math.hypot(aRe[k] as number, aIm[k] as number);
  return out;
}
