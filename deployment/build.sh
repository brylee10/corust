#!/bin/bash
# This script was taken from: 
# https://github.com/rust-lang/rust-playground/blob/main/compiler/build.sh

set -eux -o pipefail

channels_to_build="${CHANNELS_TO_BUILD-stable beta nightly}"
platform="${PLATFORM:-linux/amd64}"

repository=brylee10

for channel in $channels_to_build; do
    image_name="rust-${channel}"
    full_name="${repository}/${image_name}"

    docker build \
            --no-cache \
            --platform "${platform}" \
            -t "${full_name}" \
            --build-arg CHANNEL="${channel}" \
            ../rust
done
