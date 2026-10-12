const { getDefaultConfig } = require('expo/metro-config');

const config = getDefaultConfig(__dirname);

// Inline requires: a module's imports are required where they are first used
// rather than when the module evaluates, so startup only pays for what the
// first screen touches. Bare side-effect imports stay in place.
const defaultTransformOptions = config.transformer.getTransformOptions;
config.transformer.getTransformOptions = async (...args) => {
  const options = await defaultTransformOptions?.(...args);
  return {
    ...options,
    transform: { ...options?.transform, inlineRequires: true },
  };
};

module.exports = config;
