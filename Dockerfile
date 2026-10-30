# syntax=docker/dockerfile:1.7
# Imagem só de produção (desenho §16): sem estágio de desenvolvimento.

ARG RUST_VERSION=1.98.0
ARG LITESTREAM_VERSION=0.5.17

FROM rust:${RUST_VERSION}-bookworm AS chef
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl pkg-config \
    && rm -rf /var/lib/apt/lists/* \
    && cargo install cargo-chef --version 0.1.78 --locked

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Camada de dependências (cargo-chef) cacheada pelo Buildx; só o crate é recompilado.
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --bin engramind

FROM litestream/litestream:${LITESTREAM_VERSION} AS litestream

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system engramind \
    && useradd --system --gid engramind --no-create-home engramind \
    && mkdir -p /data \
    && chown engramind:engramind /data
COPY --from=builder /app/target/release/engramind /usr/local/bin/engramind
RUN ln -s /usr/local/bin/engramind /usr/local/bin/eng
COPY --from=litestream /usr/local/bin/litestream /usr/local/bin/litestream
COPY litestream.yml /etc/litestream.yml
COPY docker/entrypoint.sh /usr/local/bin/engramind-entrypoint
ENV DATABASE_PATH=/data/engramind.db \
    MODELS_PATH=/data/models
USER engramind:engramind
VOLUME ["/data"]
EXPOSE 8000
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:${APP_PORT:-8000}/health/live > /dev/null || exit 1
ENTRYPOINT ["/usr/local/bin/engramind-entrypoint"]
