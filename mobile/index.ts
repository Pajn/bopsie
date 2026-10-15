import './src/lib/benchEntry';
// Theme configuration must run before any route module creates a stylesheet.
import './src/ui/unistyles';
import 'expo-router/entry';
import { mark } from './src/lib/bench';

mark('modules-loaded');
