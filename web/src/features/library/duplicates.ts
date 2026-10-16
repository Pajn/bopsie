import type { DuplicateFile } from '@bopsie/client/api/schemas/admin';

/** Tracks whose copies sit in the same folders: one decision, such as deleting a folder. */
export interface FolderGroup {
  /** The folders holding a copy of each, in the order the server lists the files. */
  folders: string[];
  tracks: DuplicateFile[];
}

const folderOf = (file: string) => file.slice(0, Math.max(0, file.lastIndexOf('/'))) || '/';

/** Duplicates grouped by the folders their copies are in, the largest group first. */
export function byFolders(duplicates: DuplicateFile[]): FolderGroup[] {
  const groups = new Map<string, FolderGroup>();
  for (const duplicate of duplicates) {
    const folders = [...new Set(duplicate.files.map(folderOf))];
    const key = [...folders].sort().join('\n');
    const group = groups.get(key) ?? { folders, tracks: [] };
    group.tracks.push(duplicate);
    groups.set(key, group);
  }
  return [...groups.values()].sort((a, b) => b.tracks.length - a.tracks.length);
}
