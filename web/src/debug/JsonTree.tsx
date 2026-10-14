'use dom';

import type { DOMProps } from 'expo/dom';

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

/** The theme's colours: a DOM component is outside unistyles, so it is handed them. */
export interface TreeColors {
  text: string;
  muted: string;
  string: string;
  value: string;
}

/**
 * Raw state as a collapsible tree. A DOM component: the browser's own `<details>` does the
 * folding, and text selects and copies as it would on any page.
 */
export default function JsonTree({
  value,
  colors,
}: {
  value: Json;
  colors: TreeColors;
  dom?: DOMProps;
}) {
  return (
    <div
      style={{
        fontFamily: 'ui-monospace, monospace',
        fontSize: 12,
        lineHeight: '18px',
        color: colors.text,
      }}
    >
      <Node value={value} colors={colors} open />
    </div>
  );
}

function Node({
  name,
  value,
  colors,
  open,
}: {
  name?: string;
  value: Json;
  colors: TreeColors;
  open?: boolean;
}) {
  const label = name === undefined ? null : <span style={{ color: colors.muted }}>{name}: </span>;
  if (value === null || typeof value !== 'object') {
    return (
      <div>
        {label}
        <span style={{ color: typeof value === 'string' ? colors.string : colors.value }}>
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
        <span style={{ color: colors.muted }}>{summary}</span>
      </summary>
      <div style={{ paddingLeft: 16 }}>
        {entries.map(([key, item]) => (
          <Node key={key} name={key} value={item} colors={colors} />
        ))}
      </div>
    </details>
  );
}
