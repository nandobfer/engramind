# syntax=docker/dockerfile:1.7
# Imagem só de produção (desenho §16): sem estágio de desenvolvimento.

ARG RUST_VERSION=1.98.0
ARG LITESTREAM_VERSION=0.5.17
# ONNX Runtime do provedor local de embeddings (007): versão fixa, compatível com o `ort`
# 2.0.0-rc.15 do `fastembed` (exige 1.24 ou mais nova), conferida por SHA-256.
ARG ONNX_RUNTIME_VERSION=1.31.1
ARG ONNX_RUNTIME_SHA256=c849e36a1ef3e22d4ff217b9d1ce4a3cf5b9467b975610913ae959ad5b16bdc9

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

FROM chef AS onnxruntime
ARG ONNX_RUNTIME_VERSION
ARG ONNX_RUNTIME_SHA256
RUN curl --fail --silent --show-error --location --output /tmp/onnxruntime.tgz \
        "https://github.com/microsoft/onnxruntime/releases/download/v${ONNX_RUNTIME_VERSION}/onnxruntime-linux-x64-${ONNX_RUNTIME_VERSION}.tgz" \
    && echo "${ONNX_RUNTIME_SHA256}  /tmp/onnxruntime.tgz" | sha256sum --check - \
    && mkdir -p /opt/onnxruntime \
    && tar -xzf /tmp/onnxruntime.tgz -C /opt/onnxruntime --strip-components=2 \
        "onnxruntime-linux-x64-${ONNX_RUNTIME_VERSION}/lib/libonnxruntime.so.${ONNX_RUNTIME_VERSION}"

FROM debian:bookworm-slim AS runtime
ARG ONNX_RUNTIME_VERSION
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
COPY --from=onnxruntime /opt/onnxruntime/libonnxruntime.so.${ONNX_RUNTIME_VERSION} /usr/local/lib/libonnxruntime.so.${ONNX_RUNTIME_VERSION}
COPY litestream.yml /etc/litestream.yml
COPY docker/entrypoint.sh /usr/local/bin/engramind-entrypoint
ENV DATABASE_PATH=/data/engramind.db \
    MODELS_PATH=/data/models \
    ORT_DYLIB_PATH=/usr/local/lib/libonnxruntime.so.${ONNX_RUNTIME_VERSION}
USER engramind:engramind
VOLUME ["/data"]
EXPOSE 8000
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:${APP_PORT:-8000}/health/live > /dev/null || exit 1
ENTRYPOINT ["/usr/local/bin/engramind-entrypoint"]
