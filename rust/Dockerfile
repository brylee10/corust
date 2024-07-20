# Based on Rust Playground Dockerfile
# https://github.com/rust-lang/rust-playground/blob/main/compiler/base/Dockerfile

FROM ubuntu:20.04 as toolchain

LABEL description="Corust containerized code execution environment."

# Install packages in non-interactive mode
ENV DEBIAN_FRONTEND="noninteractive"

RUN apt-get update && apt-get install -y \
    build-essential \
    curl

# Create new user and disable root password
RUN useradd -m corust -d /corust
RUN usermod -p '!!' root 

USER corust
ENV USER=corust
ENV PATH=/corust/.cargo/bin:$PATH

# Install Rust (currently only supports stable)
# Pre-installs common components: https://rust-lang.github.io/rustup/concepts/components.html
# Layer is ~1GB in size and provides cargo, rustc, rustup, rustfmt, etc. 
RUN curl https://sh.rustup.rs -sSf | sh -s -- \
    -y \
    --profile minimal \
    --default-toolchain "stable" \
    --target wasm32-unknown-unknown \
    --component rustfmt \
    --component clippy \
    --component rust-src

# Install the `runner` binary which copies user code into the container and runs it 
COPY --chown=corust sandbox /sandbox
RUN cargo install --path /sandbox
COPY --chown=corust entrypoint.sh /corust/tools/
# Make the entrypoint script executable
RUN chmod +x /corust/tools/entrypoint.sh

# In this case the multi-stage docker build does not reduce image size because `toolchain` uses 
# all prior images
FROM toolchain

# Copy the `Cargo.toml` and build the Rust project in which user code will be run
WORKDIR /corust
RUN cargo init /corust
COPY --chown=corust sandbox/Cargo.toml.sandbox /corust/Cargo.toml
RUN cargo build --release
RUN rm src/*.rs

ENTRYPOINT ["/corust/tools/entrypoint.sh"]