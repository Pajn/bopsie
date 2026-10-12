import { useLocalSearchParams } from 'expo-router';
import { PlaylistScreen } from '@/features/playlist/PlaylistScreen';

export default function PlaylistRoute() {
  const { id, provider, name } = useLocalSearchParams<{
    id: string;
    provider: string;
    name?: string;
  }>();
  return <PlaylistScreen id={id} provider={provider ?? 'library'} {...(name ? { name } : {})} />;
}
