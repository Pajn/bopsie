import { connectSpeakers } from './speakers';

/** Connects the test speakers for the whole run; they stay connected until it ends. */
export default async function globalSetup() {
  const baseUrl = process.env.TEST_SERVER_URL;
  if (!baseUrl) return;
  return connectSpeakers(baseUrl);
}
