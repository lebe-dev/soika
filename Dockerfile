# syntax=docker/dockerfile:1

# soika — multi-stage build.
#
# Stage A  builds the SvelteKit SPA (adapter-static -> web/dist).
# Stage B  builds the Rust release binary on musl (web/dist embedded via
#          rust-embed), then compresses it with `upx -9 --lzma`.
# Runtime  alpine:3.23, rootless (uid/gid 10001), single static-ish binary.
#
# Image: tinyops/soika  —  exposes 8080, SQLite db on a mounted volume.

# ---------------------------------------------------------------------------
# Stage A — frontend (SvelteKit + Tailwind + shadcn-svelte, yarn)
# ---------------------------------------------------------------------------
FROM node:22-alpine AS frontend
WORKDIR /build/frontend

# Install deps first for layer caching (only re-runs when manifests change).
COPY frontend/package.json frontend/yarn.lock ./
RUN yarn install --frozen-lockfile

# Build the SPA. adapter-static writes to ../web/dist (see svelte.config.js),
# so the output lands at /build/web/dist for the Rust stage to embed.
COPY frontend/ ./
RUN yarn build

# ---------------------------------------------------------------------------
# Stage B — backend (Rust release on musl + UPX compression)
# ---------------------------------------------------------------------------
FROM rust:1-alpine AS backend
WORKDIR /build

# Build toolchain for the C dependencies of `ring` (rustls TLS backend) and
# static linking against musl, plus UPX for the final binary compression.
RUN apk add --no-cache build-base musl-dev upx

# Cache the dependency graph: copy manifests, fetch crates.
COPY Cargo.toml Cargo.lock ./
RUN cargo fetch

# Bring in the sources, migrations (read by sqlx::migrate! at compile time),
# and the freshly built frontend assets (embedded via rust-embed #[folder]).
COPY src/ ./src/
COPY migrations/ ./migrations/
COPY --from=frontend /build/web/dist ./web/dist

# Release build (profile.release: opt-level=z, lto, strip — see Cargo.toml).
RUN cargo build --release --locked \
    && cp target/release/soika /build/soika

# Shrink the binary as far as it will go.
RUN upx -9 --lzma /build/soika

# ---------------------------------------------------------------------------
# Runtime — minimal Alpine, rootless
# ---------------------------------------------------------------------------
FROM alpine:3.23 AS runtime

# wget (busybox) backs the HEALTHCHECK; ca-certificates for outbound SMTP TLS.
RUN apk add --no-cache ca-certificates wget \
    && addgroup -g 10001 -S soika \
    && adduser -u 10001 -S -G soika -h /data soika \
    && mkdir -p /data \
    && chown -R 10001:10001 /data

COPY --from=backend /build/soika /usr/local/bin/soika

# SQLite database lives on a mounted volume (the reference deployment).
ENV DATABASE_URL="sqlite:///data/soika.db?mode=rwc" \
    BIND_ADDR="0.0.0.0:8080"
VOLUME ["/data"]

USER 10001:10001
WORKDIR /data
EXPOSE 8080

# Liveness probe — the binary serves GET /healthz (src/router.rs).
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --quiet --spider --tries=1 http://127.0.0.1:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/soika"]
