# ---- Stage 1: Build the SvelteKit dashboard ----
FROM node:20-alpine AS ui-builder
WORKDIR /ui
COPY dashboard-ui/package*.json ./
RUN npm ci
COPY dashboard-ui/ ./
RUN npm run build

# ---- Stage 2: Build the Rust binary ----
FROM rust:1-bookworm AS rust-builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency compilation separately from application code
COPY Cargo.toml Cargo.lock ./
COPY migrations/ ./migrations/
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release \
    && rm -f target/release/deps/tus_server*

COPY src/ ./src/
RUN touch src/main.rs && cargo build --release

# ---- Stage 3: Minimal runtime image ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    curl \
    gosu \
    # ClamAV — provides clamscan and freshclam for the AV processor.
    # Signatures (~270 MB) are downloaded at runtime into a mounted volume,
    # so they are NOT baked into this image.
    clamav \
    && rm -rf /var/lib/apt/lists/* \
    # Remove the default freshclam.conf so we can pass --datadir freely.
    && rm -f /etc/clamav/freshclam.conf \
    # Create non-root service account.
    && useradd -r -u 1000 -s /sbin/nologin -c "Tuskar service" tuskar

WORKDIR /app

COPY --from=rust-builder /app/target/release/tus-server ./
COPY --from=ui-builder   /ui/build                       ./dashboard-ui/build/
COPY docker-entrypoint.sh ./

VOLUME ["/data", "/uploads"]

ENV DATABASE_URL=/data/tus.db
ENV STORAGE_DIR=/uploads
ENV BASE_URL=http://localhost:3000
ENV BIND_ADDR=0.0.0.0:3000
ENV RUST_LOG=info

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fs http://localhost:3000/api/health || exit 1

ENTRYPOINT ["./docker-entrypoint.sh"]
