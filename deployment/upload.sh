#!/bin/bash
# Uploads the docker images to Docker Hub

set -euv -o pipefail

repository=brylee10

for image in rust-stable rust-beta rust-nightly; do
    docker push "${repository}/${image}"
done

docker push "${repository}/corust-build-wasm"
