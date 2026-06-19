# syntax=docker/dockerfile:1
#
# Multi-stage build for the Taro server. The crate uses only runtime sqlx
# queries (no compile-time `query!` macros) and embeds its migrations into the
# binary, so the build needs no database and the runtime image ships no SQL.

# ---- build stage ----------------------------------------------------------
FROM rust:1-bookworm AS build
WORKDIR /app

# Warm the dependency layer first: build a stub crate so Cargo compiles all
# dependencies, and reuse that layer until Cargo.toml/Cargo.lock change.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && : > src/lib.rs \
    && cargo build --release --bin taro-server \
    && rm -rf src

# Real sources. `migrations*/` must be present: `sqlx::migrate!` embeds them
# into the binary at compile time (they are not read at runtime).
COPY src ./src
COPY migrations ./migrations
COPY migrations_pg ./migrations_pg
# Drop only this crate's stub artifacts (Docker COPY can set older mtimes, which
# makes cargo's fingerprint skip the rebuild) — the cached dependencies remain.
RUN rm -rf target/release/taro-server \
        target/release/deps/taro_server-* target/release/deps/taro-server-* \
        target/release/.fingerprint/taro-server-* target/release/.fingerprint/taro_server-* \
    && cargo build --release --bin taro-server

# ---- runtime stage --------------------------------------------------------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /app/target/release/taro-server /usr/local/bin/taro-server

# POC defaults; override with -e / compose. Blobs persist under the /data volume.
ENV TARO_BIND=0.0.0.0:8080 \
    TARO_BLOB_ROOT=/data/blobs \
    RUST_LOG=taro_server=info,tower_http=info
EXPOSE 8080
VOLUME ["/data"]

# A bare `docker run` (no DB url) falls back to SQLite under /data.
ENV TARO_DATABASE_URL=sqlite:///data/taro.db
CMD ["taro-server"]
