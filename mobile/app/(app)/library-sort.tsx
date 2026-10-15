import type { LibraryKind } from '@bopsie/client/api/endpoints/library';
import { useLocalSearchParams } from 'expo-router';
import { LibrarySortMenu } from '@/features/library/LibrarySortMenu';

export default function LibrarySortRoute() {
  const { kind } = useLocalSearchParams<{ kind: LibraryKind }>();
  return <LibrarySortMenu kind={kind} />;
}
