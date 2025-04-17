import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  webpack(config) {
    config.experiments = config.experiments || {};
    // Since Webpack 5, WebAssembly is an experimental feature, enable it
    config.experiments.asyncWebAssembly = true;
    return config;
  },
  // Remove console statements in production, except for errors
  compiler: {
    removeConsole:
      process.env.NODE_ENV === "production" ? { exclude: ["error"] } : false,
  },
};

module.exports = nextConfig;

export default nextConfig;
