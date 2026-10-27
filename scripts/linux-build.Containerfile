FROM docker.io/library/ubuntu:22.04

ARG RUST_VERSION=1.98.0
ARG NODE_VERSION=22.23.0
ARG PNPM_VERSION=11.8.0

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/node/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl wget file xz-utils pkg-config git \
        libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
        libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev libdbus-1-dev \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal \
        --default-toolchain "$RUST_VERSION" \
    && rustc --version

RUN mkdir -p /usr/local/node \
    && curl -fsSL "https://nodejs.org/dist/v$NODE_VERSION/node-v$NODE_VERSION-linux-x64.tar.xz" \
        | tar -xJ -C /usr/local/node --strip-components=1 \
    && npm install -g "pnpm@$PNPM_VERSION" \
    && node --version && pnpm --version
