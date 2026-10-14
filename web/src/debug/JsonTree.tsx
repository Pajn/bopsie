'use dom';

import type { DOMProps } from 'expo/dom';

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

/**
 * Raw state as a collapsible tree. A DOM component: the browser's own `<details>` does the
 * folding, and text selects and copies as it would on any page.
 */
export default function JsonTree({ value }: { value: Json; dom?: DOMProps }) {
  return (
    <div style={{ fontFamily: 'ui-monospace, monospace', fontSize: 12, lineHeight: '18px' }}>
      <Node value={value} open />
    </div>
  );
}

function Node({ name, value, open }: { name?: string; value: Json; open?: boolean }) {
  const label = name === undefined ? null : <span style={{ opacity: 0.7 }}>{name}: </span>;
  if (value === null || typeof value !== 'object') {
    return (
      <div>
        {label}
        <span style={{ color: typeof value === 'string' ? '#2E7D32' : '#6750A4' }}>
          {JSON.stringify(value)}
        </span>
      </div>
    );
  }
  const entries: Array<[string, Json]> = Array.isArray(value)
    ? value.map((item, i) => [String(i), item])
    : Object.entries(value);
  const summary = Array.isArray(value) ? `[${value.length}]` : `{${entries.length}}`;
  return (
    <details open={open}>
      <summary style={{ cursor: 'pointer' }}>
        {label}
        <span style={{ opacity: 0.6 }}>{summary}</span>
      </summary>
      <div style={{ paddingLeft: 16 }}>
        {entries.map(([key, item]) => (
          <Node key={key} name={key} value={item} />
        ))}
      </div>
    </details>
  );
}
