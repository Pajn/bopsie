/**
 * Integration tests run the API layer against a real server, with no React
 * Native runtime involved. Configure the server through TEST_SERVER_URL,
 * TEST_SERVER_USERNAME and TEST_SERVER_PASSWORD, or start a disposable Bopsie
 * server with `pnpm server:start`. Without TEST_SERVER_URL every suite is skipped.
 */
/** @type {import('jest').Config} */
module.exports = {
  rootDir: '../..',
  preset: 'jest-expo/node',
  testMatch: ['<rootDir>/test/integration/**/*.test.ts'],
  moduleNameMapper: { '^@/(.*)$': '<rootDir>/src/$1' },
  testTimeout: 60_000,
};
