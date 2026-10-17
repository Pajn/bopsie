import { useLocalSearchParams } from 'expo-router';
import { AudiobookScreen } from '@/features/audiobooks/AudiobookScreen';

export default function AudiobookRoute() {
  const { id, provider, name } = useLocalSearchParams<{
    id: string;
    provider: string;
    name?: string;
  }>();
  return (
    <AudiobookScreen id={id} provider={provider ?? 'audiobookshelf'} {...(name ? { name } : {})} />
  );
}
