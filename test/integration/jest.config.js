/**
 * Integration tests run the API layer against a real Music Assistant server,
 * with no React Native runtime involved. Configure the server through MA_URL,
 * MA_USERNAME and MA_PASSWORD, or start a disposable one with `pnpm ma:start`.
 * Without MA_URL every suite is skipped.
 */
/** @type {import('jest').Config} */
module.exports = {
  rootDir: '../..',
  preset: 'jest-expo/node',
  testMatch: ['<rootDir>/test/integration/**/*.test.ts'],
  moduleNameMapper: { '^@/(.*)$': '<rootDir>/src/$1' },
  testTimeout: 60_000,
};
