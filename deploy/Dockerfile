# syntax=docker/dockerfile:1.7

# PhotoTools server image.
#
# Build context is the repository root, not this directory:
#
#     docker build -f deploy/Dockerfile .
#
# Four stages. `chef` and `planner` exist so that the dependency tree — which is
# most of the build, and changes far less often than the source — becomes one
# cached layer. Without that separation every edit to a single .rs file
# recompiles rawler, mozjpeg and aws-lc from scratch, which is roughly twenty
# minutes per architecture (specification §9.3, build plan Phase 14 task 1).

ARG RUST_VERSION=1.90
ARG DEBIAN_RELEASE=bookworm

# ---------------------------------------------------------------------------
# chef — the toolchain plus cargo-chef, shared by the two stages below
# ---------------------------------------------------------------------------
FROM rust:${RUST_VERSION}-${DEBIAN_RELEASE} AS chef
# cmake and clang build aws-lc-sys (rustls' cryptography, reached through
# reqwest); nasm builds mozjpeg's SIMD kernels, without which the JPEG encoder
# still works but loses the assembly paths §9.1's timing depends on.
RUN apt-get update && apt-get install -y --no-install-recommends \
        cmake \
        clang \
        nasm \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked --version ^0.1
WORKDIR /build

# ---------------------------------------------------------------------------
# planner — reduce the workspace to a dependency recipe
# ---------------------------------------------------------------------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ---------------------------------------------------------------------------
# builder — cook the dependencies, then build the server against them
# ---------------------------------------------------------------------------
FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json

# `-p phototools-server` matters twice over. It keeps the desktop crate out of
# the image, and the desktop crate is Tauri: on Linux that means GTK and WebKit
# development headers, several hundred megabytes of build dependency for a
# binary that only ever runs on a Mac (G1 — the server binary is transport and
# process lifecycle, nothing else).
RUN cargo chef cook --release --recipe-path recipe.json -p phototools-server

COPY . .
RUN cargo build --release -p phototools-server

# ---------------------------------------------------------------------------
# frontend — the web UI the server hands to a browser
# ---------------------------------------------------------------------------
# Specification §2.2: the web UI is served by phototools-server. It is built
# here rather than committed, so the image cannot ship a bundle older than the
# API it talks to.
FROM node:22-slim AS frontend
WORKDIR /build

# Manifests first, so a source edit does not re-run three installs. The shared
# package is a `file:` dependency of both front ends, which is why its manifest
# has to be present before either install resolves.
COPY frontend/shared/package.json frontend/shared/package-lock.json frontend/shared/
COPY frontend/web/package.json frontend/web/package-lock.json frontend/web/
RUN npm --prefix frontend/shared ci && npm --prefix frontend/web ci

COPY frontend/ frontend/

# The shared client is consumed as compiled output, so it is built first; the
# web build resolves `@phototools/shared` to the dist this produces.
RUN npm --prefix frontend/shared run build \
    && npm --prefix frontend/web run build

# ---------------------------------------------------------------------------
# runtime
# ---------------------------------------------------------------------------
# Specification §9.3 asks for `distroless/cc`, and this is a deliberate
# departure from it, recorded in docs/phase-reports/phase-14.md.
#
# `exiftool` is a Perl program. §2.6 makes it the one permitted external binary
# and requires it for every metadata *write*, and two shipped server routes
# perform one: POST /api/tools/dates/fix (F1) and POST /api/ingest/derive (F14,
# which copies metadata onto each derivative). `distroless/cc` carries a C
# runtime and no interpreter, so on that base both routes fail at the first
# write — a capability the API offers and the image cannot honour.
#
# The two honest options were a Perl-bearing base, or dropping those routes from
# the server. This takes the base image, because the routes are specified.
FROM debian:${DEBIAN_RELEASE}-slim AS runtime

# curl is here for the health check and nothing else. A compose-level
# healthcheck runs *inside* this container, so the client has to be in the
# image either way; the alternative is a --health-check flag on the binary,
# which is code in a binary crate to do what one HTTP request does (G1).
RUN apt-get update && apt-get install -y --no-install-recommends \
        libimage-exiftool-perl \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/* \
    && exiftool -ver

# Nothing here needs root. The uid is fixed rather than allocated so that a bind
# mount from the NAS can be given matching ownership once and keep it across
# rebuilds.
RUN useradd --system --uid 10001 --create-home --home-dir /home/phototools phototools

COPY --from=builder /build/target/release/phototools-server /usr/local/bin/phototools-server
COPY --from=frontend /build/frontend/web/dist /srv/web

# Both are mount points, created here so a container started without volumes
# still has somewhere writable to put the ledger rather than failing at open.
RUN mkdir -p /data /library && chown phototools:phototools /data /library

USER phototools
ENV PORT=3000 \
    DATABASE_PATH=/data/phototools.db \
    STAGING_DIR=/data/staging \
    WEB_ROOT=/srv/web \
    RUST_LOG=phototools_server=info,tower_http=info

EXPOSE 3000

# /api/health is the one unauthenticated route (specification §5.3), which is
# what makes it usable as a health check without embedding a credential in the
# image. Repeated in docker-compose.yml, where an operator can see it.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:${PORT}/api/health || exit 1

ENTRYPOINT ["/usr/local/bin/phototools-server"]
