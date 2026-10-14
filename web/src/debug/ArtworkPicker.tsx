'use dom';

import { uploadArtwork } from '@bopsie/client/api/endpoints/admin';
import type { DOMProps } from 'expo/dom';
import { useState } from 'react';

/**
 * Picks a JPEG or PNG and uploads it. A DOM component, for the browser's own file picker; it
 * uploads itself and hands back only the proxy id, which crosses to the app as a string.
 */
export default function ArtworkPicker({
  serverUrl,
  token,
  color,
  onUploaded,
  onFailed,
}: {
  serverUrl: string;
  token: string;
  color: string;
  onUploaded: (proxyId: string) => void;
  onFailed: (message: string) => void;
  dom?: DOMProps;
}) {
  const [busy, setBusy] = useState(false);
  return (
    <label
      style={{
        color,
        fontFamily: 'system-ui, sans-serif',
        fontSize: 14,
        cursor: busy ? 'progress' : 'pointer',
      }}
    >
      {busy ? 'Uploading…' : 'Upload artwork (JPEG or PNG) '}
      <input
        type="file"
        accept="image/jpeg,image/png"
        disabled={busy}
        style={{ color }}
        onChange={async (event) => {
          const file = event.currentTarget.files?.[0];
          if (!file) return;
          setBusy(true);
          try {
            onUploaded(await uploadArtwork(serverUrl, token, file));
          } catch (error) {
            onFailed(error instanceof Error ? error.message : 'The upload failed');
          } finally {
            setBusy(false);
          }
        }}
      />
    </label>
  );
}
