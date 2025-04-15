import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  webpack: (config) => {
    // 1. Add .wasm to the extensions list (from your CRA config)
    config.resolve.extensions.push(".wasm");

    // Since Webpack 5 doesn't enable WebAssembly by default, we should do it manually
    config.experiments = { ...config.experiments, asyncWebAssembly: true };

    // 3. Exclude .wasm from asset/resource handling (similar to your CRA config)
    config.module.rules.forEach((rule: any) => {
      if (rule.oneOf) {
        rule.oneOf.forEach((oneOf: any) => {
          if (oneOf.type === "asset/resource") {
            oneOf.exclude = oneOf.exclude || [];
            oneOf.exclude.push(/\.wasm$/);
          }
        });
      }
    });

    return config;
  },
};

module.exports = nextConfig;

export default nextConfig;
