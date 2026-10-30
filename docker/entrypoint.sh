#!/bin/sh
# Entrypoint da imagem: restaura do Litestream quando o banco não existe e sobe o
# servidor sob `litestream replicate`. Com argumentos, executa o comando informado
# (ex.: `docker compose exec api engramind doctor`).
set -eu

if [ "$#" -gt 0 ]; then
    exec "$@"
fi

DATABASE_PATH="${DATABASE_PATH:-/data/engramind.db}"
export DATABASE_PATH

case "${LITESTREAM_ENABLED:-false}" in
    true)
        : "${ENGRAMIND_S3_BUCKET:?ENGRAMIND_S3_BUCKET is required when LITESTREAM_ENABLED=true}"
        : "${ENGRAMIND_S3_ENDPOINT:?ENGRAMIND_S3_ENDPOINT is required when LITESTREAM_ENABLED=true}"
        : "${LITESTREAM_ACCESS_KEY_ID:?LITESTREAM_ACCESS_KEY_ID is required when LITESTREAM_ENABLED=true}"
        : "${LITESTREAM_SECRET_ACCESS_KEY:?LITESTREAM_SECRET_ACCESS_KEY is required when LITESTREAM_ENABLED=true}"
        export ENGRAMIND_S3_PATH="${ENGRAMIND_S3_PATH:-engramind}"
        litestream restore -config /etc/litestream.yml -if-db-not-exists -if-replica-exists "$DATABASE_PATH"
        exec litestream replicate -config /etc/litestream.yml -exec "engramind serve"
        ;;
    false)
        exec engramind serve
        ;;
    *)
        printf '%s\n' "LITESTREAM_ENABLED invalido: use 'true' ou 'false'" >&2
        exit 1
        ;;
esac
