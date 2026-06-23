# ============================================================
# PlotWeb — single-container build
# Rinch is cloned from git at a pinned commit during build.
# ============================================================

# ------ Stage 1: Build the WASM frontend with Trunk ---------
FROM rust:1.88-bookworm AS frontend

# Pinned to rinch origin/main (includes the new ProseMirror-style rich-text
# editor, PR #65, plus the earlier rsx capture-scanner if-let fix). Repin to a
# newer origin/main commit as rinch evolves.
ARG RINCH_COMMIT=1f93dae079208b2cc1f32f0551231438dd4b6871
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

# rhypedb is a sibling repo (like rinch) referenced by path in the workspace
# Cargo.toml. It doesn't exist in the Docker build context, so clone it at a
# pinned commit and repoint the path deps below. Built with default-features
# = false (no ONNX/fastembed) — see the workspace Cargo.toml.
#
# Pinned to the tip of the `feat/optional-fastembed` branch, which makes the
# fastembed/ONNX stack an opt-in feature — REQUIRED for our default-features=false
# build. rhypedb master does NOT have this yet; repin to the squash-merge commit
# on master once that work lands.
ARG RHYPEDB_REPO=https://github.com/joeleaver/rhypedb.git
ARG RHYPEDB_COMMIT=680de58d2d72e2b775b20677025de8a629d9cac8

RUN apt-get update && apt-get install -y --no-install-recommends git \
    && rm -rf /var/lib/apt/lists/*

# Clone rhypedb at the pinned commit
RUN git clone "$RHYPEDB_REPO" /build/rhypedb \
    && cd /build/rhypedb && git checkout "$RHYPEDB_COMMIT"

COPY . /build/plotweb/
COPY --from=frontend /build/plotweb/plotweb-web/dist/ /build/plotweb/plotweb-web/dist/

WORKDIR /build/plotweb

# Patch rhypedb paths for the Docker build context (preserves the trailing
# crate path and `, default-features = false }`; only the path prefix changes).
RUN sed -i 's|path = "../../personal/rhypedb/|path = "/build/rhypedb/|g' Cargo.toml

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
