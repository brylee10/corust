# Based on: https://github.com/aws-samples/aws-amplify-webassembly
# Compiles WASM and builds/starts UI
build-ci-image:
	@docker build -t corust-wasm-build-image ./deployment

# Use default `--target bundler` for webpack, not `--target web` which requires 
# manually init wasm
# https://rustwasm.github.io/docs/wasm-pack/commands/build.html#target
build-wasm:
	@wasm-pack build --target bundler --release ../rust/components

# Create optimized production build
deploy: build-wasm link
	@npm run build

# Start local dev server
start: build-wasm link
	@npm start

# `corust_components` is a local npm package that is linked to `node_modules`, 
# not installed from npm registry
link:
	@cd ../rust/components/pkg && npm link
	@npm link corust-components
