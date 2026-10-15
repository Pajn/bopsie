// Imported first by the app entry so this mark lands before any other module
// evaluates; imports are hoisted, so a call in the entry file itself would
// only run once everything it imports has loaded.
import { mark } from './bench';

mark('js-entry');
