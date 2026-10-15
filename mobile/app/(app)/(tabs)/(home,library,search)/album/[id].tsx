import { useLocalSearchParams } from 'expo-router';
import { AlbumScreen } from '@/features/album/AlbumScreen';

export default function AlbumRoute() {
  const { id, provider, name } = useLocalSearchParams<{
    id: string;
    provider: string;
    name?: string;
  }>();
  return <AlbumScreen id={id} provider={provider ?? 'library'} {...(name ? { name } : {})} />;
}
