// Packages that ship untranspiled ESM or Flow and must go through Babel. pnpm
// stores packages under node_modules/.pnpm/<name>@<version> with `/` in scoped
// names replaced by `+`, so both separators are accepted.
const transformPackages = [
  '(jest-)?react-native',
  '@react-native(-community)?[/+]',
  'expo(nent)?',
  '@expo(nent)?[/+]',
  'react-navigation',
  '@react-navigation[/+]',
  '@legendapp[/+]',
  'react-native-unistyles',
  'react-native-nitro-modules',
].join('|');

/** @type {import('jest').Config} */
module.exports = {
  preset: 'jest-expo',
  testMatch: [
    '<rootDir>/src/**/*.test.ts',
    '<rootDir>/src/**/*.test.tsx',
    '<rootDir>/scripts/**/*.test.ts',
  ],
  moduleNameMapper: { '^@/(.*)$': '<rootDir>/src/$1' },
  transformIgnorePatterns: [`node_modules/(?!(\\.pnpm/)?(${transformPackages}))`],
};
