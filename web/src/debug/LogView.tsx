'use dom';

import type { DOMProps } from 'expo/dom';
import { useEffect, useRef } from 'react';

export interface LogViewLine {
  seq: number;
  time: string;
  level: string;
  target: string;
  text: string;
}

export interface LogColors {
  text: string;
  muted: string;
  error: string;
  warn: string;
  background: string;
}

/**
 * Log lines in a scrolling monospace block. A DOM component: the browser lays out and
 * selects thousands of lines of text far better than views do. It stays at the bottom while
 * the reader is there, and holds still while they scroll back.
 */
export default function LogView({
  lines,
  colors,
}: {
  lines: LogViewLine[];
  colors: LogColors;
  dom?: DOMProps;
}) {
  const box = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const last = lines.at(-1)?.seq;

  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll when a new line arrives.
  useEffect(() => {
    if (pinned.current && box.current) box.current.scrollTop = box.current.scrollHeight;
  }, [last]);

  const color = (level: string) =>
    level === 'ERROR' ? colors.error : level === 'WARN' ? colors.warn : colors.text;

  return (
    <div
      ref={box}
      onScroll={(event) => {
        const el = event.currentTarget;
        pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
      }}
      style={{
        height: '70vh',
        overflowY: 'auto',
        fontFamily: 'ui-monospace, monospace',
        fontSize: 12,
        lineHeight: '18px',
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
        background: colors.background,
        padding: 8,
        borderRadius: 8,
      }}
    >
      {lines.map((line) => (
        <div key={line.seq} style={{ color: color(line.level) }}>
          <span style={{ color: colors.muted }}>{line.time} </span>
          {line.level.padEnd(5)} <span style={{ color: colors.muted }}>{line.target}</span>{' '}
          {line.text}
        </div>
      ))}
    </div>
  );
}
