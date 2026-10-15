import { useLocalSearchParams } from 'expo-router';
import { ItemMenu } from '@/features/library/ItemMenu';
import type { MenuKind } from '@/features/library/useOpenItemMenu';

export default function ItemMenuRoute() {
  const { kind, id, provider } = useLocalSearchParams<{
    kind: MenuKind;
    id: string;
    provider: string;
  }>();
  return <ItemMenu kind={kind} id={id} provider={provider ?? 'library'} />;
}
