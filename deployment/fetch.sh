#!/bin/bash
# This script was taken from:
# https://github.com/rust-lang/rust-playground/blob/main/compiler/fetch.sh

set -euv -o pipefail

repository=brylee10

for image in rust-stable rust-beta rust-nightly; do
    docker pull "${repository}/${image}"
    # The backend expects images without a repository prefix
    docker tag "${repository}/${image}" "${image}"
done