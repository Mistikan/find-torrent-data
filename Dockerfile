# syntax=docker/dockerfile:1

FROM rust:bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates screen \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/find-torrent-data /usr/local/bin/
ENTRYPOINT ["find-torrent-data"]
