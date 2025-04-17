# Based on: https://github.com/aws-samples/aws-amplify-webassembly
# Compiles WASM and builds/starts UI
build-ci-image:
	@docker build -t corust-wasm-build-image ./deployment

# Use default `--target bundler` for webpack, not `--target web` which requires 
# manually init wasm
# https://rustwasm.github.io/docs/wasm-pack/commands/build.html#target
build-wasm:
	@wasm-pack build --target bundler --release ../rust/components

# Create the optimized production build
build: build-wasm link
	@npm run build

# Run optimized production build
start: build-wasm link
	@npm run start

# Start local dev server
dev: build-wasm link
	@npm run dev

# Install dependencies.
# Instead of treating `corust_components` as a local npm package that is linked to `node_modules`, 
# directly copy the contents into `node_modules`. CRA supported linking, but NextJS is having some trouble.
link:
	@rsync -a --delete ../rust/components/pkg/ ./node_modules/corust-components/
