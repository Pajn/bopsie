/** Plain TypeScript with no React Native runtime, as the integration suites run. */
/** @type {import('jest').Config} */
module.exports = {
  preset: 'jest-expo/node',
  testMatch: ['<rootDir>/src/**/*.test.ts'],
};
