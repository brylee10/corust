#!/bin/bash
# This script was taken from: 
# https://github.com/rust-lang/rust-playground/blob/main/compiler/build.sh

set -eux -o pipefail

channels_to_build="${CHANNELS_TO_BUILD-stable beta nightly}"
platform="${PLATFORM:-linux/amd64}"
# Optionally build the wasm image (configures the Amplify build environment)
build_wasm_image="${BUILD_WASM_IMAGE:-true}"

repository=brylee10

for channel in $channels_to_build; do
    image_name="rust-${channel}"
    full_name="${repository}/${image_name}"

    docker build \
            --platform "${platform}" \
            -t "${image_name}" \
            -t "${full_name}" \
            --build-arg CHANNEL="${channel}" \
            ../rust
done

if [ "${build_wasm_image}" = true ]; then
    image_name="corust-build-wasm"
    full_name="${repository}/${image_name}"

    docker build --platform "${platform}" -t "${image_name}" -t "${full_name}" ../deployment
fi
