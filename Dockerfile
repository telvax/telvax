# syntax=docker/dockerfile:1
# The `telvax` and `tvpm` binaries on Debian 12. Published as ghcr.io/telvax/telvax by the release
# workflow. Run a program: docker run --rm -v "$PWD:/work" -w /work ghcr.io/telvax/telvax run app.tvx main

FROM rust:slim-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*
# A minimal nightly; RUSTUP_TOOLCHAIN overrides rust-toolchain.toml so no other component is fetched.
RUN rustup toolchain install nightly --profile minimal
ENV RUSTUP_TOOLCHAIN=nightly
WORKDIR /telvax
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/telvax/target \
    cargo build --profile dist --locked -p telvax_cli -p telvax_pkg \
    && cp target/dist/telvax target/dist/tvpm /usr/local/bin/

FROM debian:bookworm-slim
LABEL org.opencontainers.image.source="https://github.com/telvax/telvax" \
      org.opencontainers.image.description="The Telvax compiler and package manager" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"
RUN apt-get update && apt-get install -y --no-install-recommends libssl3 ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/bin/telvax /usr/local/bin/tvpm /usr/local/bin/
ENTRYPOINT ["telvax"]
