/* config-overrides.js */

module.exports = function override(webpackConfig, env) {
  // By default, CRA does not resolve WASM
  webpackConfig.resolve.extensions.push(".wasm");
  // Since webpack 5 WebAssembly is not enabled by default and flagged as experimental feature
  webpackConfig.experiments = {
    asyncWebAssembly: true,
  };
  webpackConfig.module.rules.forEach((rule) => {
    (rule.oneOf ?? []).forEach((oneOf) => {
      if (oneOf.type === "asset/resource") {
        // Exclude `wasm` extensions so they get processed by webpacks internal loaders.
        oneOf.exclude.push(/\.wasm$/);
      }
    });
  });
  return webpackConfig;
};
