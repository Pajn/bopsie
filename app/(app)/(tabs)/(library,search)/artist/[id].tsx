import { useLocalSearchParams } from 'expo-router';
import { ArtistScreen } from '@/features/artist/ArtistScreen';

export default function ArtistRoute() {
  const { id, provider, name } = useLocalSearchParams<{
    id: string;
    provider: string;
    name?: string;
  }>();
  return <ArtistScreen id={id} provider={provider ?? 'library'} {...(name ? { name } : {})} />;
}
