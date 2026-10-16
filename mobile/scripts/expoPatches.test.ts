/** @jest-environment node */

// Exercise the installed patches so an SDK or patch update cannot silently drop them.
const { transformSync } = require('@babel/core');
const {
  importExportLiveBindingsPlugin,
} = require('@expo/metro-config/build/transform-plugins/importExportLiveBindings');

jest.mock('@expo/cli/build/src/api/getVersions', () => ({
  getVersionsAsync: async () => ({
    sdkVersions: {
      '58.0.0': { relatedPackages: { jest: '~29.7.0', '@types/jest': '29.5.14' } },
      '57.0.0': { relatedPackages: { jest: '~29.7.0', '@types/jest': '29.5.14' } },
    },
  }),
}));
jest.mock('@expo/cli/build/src/start/doctor/dependencies/bundledNativeModules', () => ({
  getVersionedNativeModulesAsync: async () => ({}),
}));
jest.mock('@expo/cli/build/src/start/doctor/dependencies/resolvePackages', () => ({
  hasExpoCanaryAsync: async () => false,
}));

const {
  getCombinedKnownVersionsAsync,
} = require('@expo/cli/build/src/start/doctor/dependencies/getVersionedPackages');

it('keeps Jest 30 compatible with SDK 58 despite stale remote recommendations', async () => {
  const versions = await getCombinedKnownVersionsAsync({ projectRoot: '.', sdkVersion: '58.0.0' });
  expect(versions.jest).toBe('^30.0.0');
  expect(versions['@types/jest']).toBe('^30.0.0');
  const previous = await getCombinedKnownVersionsAsync({ projectRoot: '.', sdkVersion: '57.0.0' });
  expect(previous.jest).toBe('~29.7.0');
});

function compile(source: string, importDefault: string | null = 'importDefault') {
  return transformSync(source, {
    babelrc: false,
    configFile: false,
    plugins: [[importExportLiveBindingsPlugin, { importDefault, performConstantFolding: false }]],
  }).code;
}

it('uses Metro default imports directly and keeps re-exports live', () => {
  const code = compile(
    'import value from "fixture"; export { value, default as other } from "fixture"; export default value;',
  );
  const exports: Record<string, unknown> = {};
  const module = { __esModule: true, default: { title: 'track' }, value: 'named' };
  const loadDefault = jest.fn(() => module.default);
  new Function('exports', 'require', 'importDefault', code)(exports, () => module, loadDefault);
  expect(exports.default).toBe(module.default);
  expect(exports.other).toBe(module.default);
  expect(exports.value).toBe('named');
  module.value = 'updated';
  expect(exports.value).toBe('updated');
  expect(loadDefault).toHaveBeenCalledTimes(1);
});

it('preserves bare import order and the fallback without Metro helpers', () => {
  const code = compile('import "first"; import value from "second"; export default value;');
  const order: string[] = [];
  const exports: Record<string, unknown> = {};
  new Function('exports', 'require', 'importDefault', code)(
    exports,
    (name: string) => order.push(name),
    (name: string) => {
      order.push(name);
      return 'loaded';
    },
  );
  expect(order).toEqual(['first', 'second']);
  expect(exports.default).toBe('loaded');
  const fallback = compile('import value from "fixture"; export default value;', null);
  const commonjs = { title: 'track' };
  const fallbackExports: Record<string, unknown> = {};
  new Function('exports', 'require', fallback)(fallbackExports, () => commonjs);
  expect(fallbackExports.default).toBe(commonjs);
});
