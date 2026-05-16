# syntax=docker/dockerfile:1

FROM rust:bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:bookworm-slim AS runtime

RUN groupadd -g 1000 find-torrent-data && \
    useradd -u 1000 -g 1000 -m find-torrent-data

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates screen jq nano fdupes \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/find-torrent-data /usr/local/bin/

USER find-torrent-data

ENTRYPOINT ["find-torrent-data"]
