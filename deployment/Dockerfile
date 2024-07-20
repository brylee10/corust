# This Dockerfile is used as a custom build image for AWS Amplify, particularly to compile 
# Rust code to WebAssembly.
# 
# This Dockerfile was largely taken from the below, with modifications: 
# https://github.com/aws-samples/aws-amplify-webassembly/blob/main/ci-build-image/Dockerfile
#
# To give credits to the original authors, the following license is maintained. 
# The original authors are removed to clarify any errors due to modifications are from the author 
# of this repository. 

#!
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: MIT-0
#

#    IMPORTANT:
#
#    To be able to use a custom container image as a build environment in AWS Amplify,
#    it will need to have the following tools available inside:
#
#    1. `curl`
#    2. `git`
#    3. the OpenSSH package
#
#
#    Additionally, if the build will use Node.js and `npm` in the process (as is this repository),
#    those will also need to be pre-installed in the image.
#
#    @see https://docs.aws.amazon.com/amplify/latest/userguide/custom-build-image.html#setup

# :: -----------------------------------------------------------------------------------

# :: What tools you install in the image will depend significantly on what you use
#    to build out your WASM binaries. In this repository, we're using Rust and wasm-pack
#    for our WASM tooling, so we'll install those accordingly.
# 
#    Make sure to adjust based on your particular needs.

FROM node:22.3-slim

# :: Rust will refers to these env vars.
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

# :: Update + install necessary tooling first (see above).
# Installs additional `tar` to unpack wasm-opt.
RUN apt update \
    && apt install -y curl git \
    && apt install -y --no-install-recommends ca-certificates gcc libc6-dev make \
    && apt install -y tar

# :: Install rust via the rustup script.
#    This will install both the Rust compiler (rustc) and Cargo (cargo).
#    @see https://rustup.rs
RUN curl --proto '=https' --tlsv1.2 -sSf --output rustup https://sh.rustup.rs \
    && chmod +x ./rustup \
    && ./rustup -y --no-modify-path --default-toolchain nightly \
    && chmod -R a+w $RUSTUP_HOME $CARGO_HOME

# :: Install wasm-pack via wasm-pack's init script.
#    @see https://rustwasm.github.io/wasm-pack/installer
RUN curl --proto '=https' --tlsv1.2 -sSf --output wasm-pack-init https://rustwasm.github.io/wasm-pack/installer/init.sh \
    && chmod +x ./wasm-pack-init \
    && ./wasm-pack-init

# Install wasm-opt version 117 via binaryen
RUN curl -L https://github.com/WebAssembly/binaryen/releases/download/version_117/binaryen-version_117-x86_64-linux.tar.gz -o binaryen-version_117-x86_64-linux.tar.gz  \
    && tar -xvf binaryen-version_117-x86_64-linux.tar.gz \
    && mv binaryen-version_117 binaryen \
    && mv binaryen/bin/* /usr/local/bin/ \
    && rm -rf binaryen-version_117-x86_64-linux.tar.gz \
    && rm -rf binaryen

# :: Perform various cleanup tasks.
RUN rm ./rustup ./wasm-pack-init \
    rm -rf /var/lib/apt/lists/*、

RUN echo export PATH="$PATH" >> ~/.bashrc

ENTRYPOINT ["bash", "-c"]
