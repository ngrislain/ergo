# syntax=docker/dockerfile:1
ARG RUST_VERSION=1.97.1

FROM --platform=$BUILDPLATFORM docker.io/library/rust:${RUST_VERSION}-bookworm AS web-build
ARG BUILDARCH
ARG WASM_BINDGEN_VERSION=0.2.129
RUN rustup target add wasm32-unknown-unknown
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install wasm-bindgen-cli --version "${WASM_BINDGEN_VERSION}" --locked
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY crates crates
COPY web web
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=ergo-web-${BUILDARCH},target=/src/target \
    cargo xtask build-web

FROM --platform=$BUILDPLATFORM docker.io/library/rust:${RUST_VERSION}-bookworm AS server-build
ARG TARGETARCH
# Compile on the builder's native CPU; emulated rustc/LLVM can fail on ARM Macs.
# Debian's native AMD64 GCC also provides the x86_64-linux-gnu-prefixed tools.
RUN if [ "$(uname -m)" != "x86_64" ]; then \
      apt-get update && apt-get install -y --no-install-recommends gcc-x86-64-linux-gnu libc6-dev-amd64-cross \
      && rm -rf /var/lib/apt/lists/*; \
    fi
RUN rustup target add x86_64-unknown-linux-gnu
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
    AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY crates crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=ergo-server-${TARGETARCH},target=/src/target \
    cargo build --release --locked --target x86_64-unknown-linux-gnu -p ergo-server && \
    cp target/x86_64-unknown-linux-gnu/release/ergo-server /ergo-server

FROM docker.io/library/debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=server-build /ergo-server /usr/local/bin/ergo-server
COPY --from=web-build /src/web /app/web
ENV ERGO_BIND=0.0.0.0:8080 ERGO_DATA_DIR=/tmp/ergo
USER 10001:10001
EXPOSE 8080
ENTRYPOINT ["ergo-server"]
