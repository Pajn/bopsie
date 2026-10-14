/** @type {import('jest').Config} */
module.exports = {
  preset: 'jest-expo/web',
  testMatch: ['<rootDir>/src/**/*.test.ts', '<rootDir>/src/**/*.test.tsx'],
  moduleNameMapper: { '^@/(.*)$': '<rootDir>/src/$1' },
};
