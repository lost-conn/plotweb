# ============================================================
# PlotWeb — single-container build
# Rinch is cloned from git at a pinned commit during build.
# ============================================================

# ------ Stage 1: Build the WASM frontend with Trunk ---------
FROM rust:1.88-bookworm AS frontend

ARG RINCH_COMMIT=fd3f7d76ebb671d35c14e9a39cfc9297b6824721
ARG RINCH_REPO=https://github.com/joeleaver/rinch.git

RUN apt-get update && apt-get install -y --no-install-recommends git \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-unknown-unknown \
    && cargo install trunk --locked

# Clone rinch at the pinned commit
RUN git clone "$RINCH_REPO" /build/rinch \
    && cd /build/rinch && git checkout "$RINCH_COMMIT"

# Copy plotweb source
COPY . /build/plotweb/

WORKDIR /build/plotweb/plotweb-web

# Patch rinch paths for the Docker build context
RUN sed -i 's|path = "../../rinch/|path = "/build/rinch/|g' Cargo.toml

RUN trunk build --release

# ------ Stage 2: Build the Rust backend --------------------
FROM rust:1.88-bookworm AS backend

COPY . /build/plotweb/
COPY --from=frontend /build/plotweb/plotweb-web/dist/ /build/plotweb/plotweb-web/dist/

WORKDIR /build/plotweb
RUN cargo build --release --package plotweb-server

# ------ Stage 3: Minimal runtime image ---------------------
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash plotweb \
    && mkdir -p /home/plotweb/data/books \
    && chown -R plotweb:plotweb /home/plotweb

USER plotweb
WORKDIR /home/plotweb

COPY --chown=plotweb:plotweb --from=backend /build/plotweb/target/release/plotweb-server ./plotweb-server
COPY --chown=plotweb:plotweb --from=frontend /build/plotweb/plotweb-web/dist/ ./dist/

ENV DIST_DIR=/home/plotweb/dist
ENV DATA_DIR=/home/plotweb/data/books
ENV DATABASE_URL=sqlite:/home/plotweb/plotweb.db

EXPOSE 3000

CMD ["./plotweb-server"]
