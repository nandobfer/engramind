#!/usr/bin/env sh
# Valida o docker-compose.yml (e o override de deploy) sem subir nada.
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$SCRIPT_DIR/.."

fail() {
    printf '%s\n' "$1" >&2
    exit 1
}

# Valores fictícios só para a interpolação; nunca são usados para subir nada.
APP_PORT=8317
CONTAINER_NAME=compose-validation-api
TOKEN_PEPPER=compose-validation-token-pepper-value
SECRETS_KEY=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=
MCP_ALLOWED_HOSTS=compose-validation-api
LOG_FORMAT=compact
LOG_ANSI=true
export APP_PORT CONTAINER_NAME TOKEN_PEPPER SECRETS_KEY MCP_ALLOWED_HOSTS LOG_FORMAT LOG_ANSI
unset API_IMAGE ENGRAMIND_NETWORK S3_NETWORK 2>/dev/null || true

docker compose -f docker-compose.yml config --quiet
config="$(docker compose -f docker-compose.yml config --format json | tr -d '[:space:]')"

case "$config" in
    '{"name":"engramind-8317",'*) ;;
    *) fail 'name must be engramind-${APP_PORT}' ;;
esac
case "$config" in
    *'"profiles"'*) fail 'docker-compose.yml must not declare profiles' ;;
esac
services="$(docker compose -f docker-compose.yml config --services)"
[ "$services" = "api" ] || fail "docker-compose.yml must declare only the api service (found: $services)"
case "$config" in
    *'"container_name":"compose-validation-api"'*) ;;
    *) fail 'api must use CONTAINER_NAME from the environment' ;;
esac
case "$config" in
    *'"image":"engramind-api:latest"'*) ;;
    *) fail 'api must default to the engramind-api:latest image' ;;
esac
case "$config" in
    *'"APP_PORT":"8317"'*'"target":8317'*'"published":"8317"'*) ;;
    *) fail 'api must publish APP_PORT from the environment' ;;
esac
case "$config" in
    *'"init":true'*) ;;
    *) fail 'api must run with init: true' ;;
esac
case "$config" in
    *'"restart":"unless-stopped"'*) ;;
    *) fail 'api must use restart: unless-stopped' ;;
esac
case "$config" in
    *'"type":"volume","source":"engramind-data","target":"/data"'*) ;;
    *) fail 'api must mount the named volume engramind-data at /data' ;;
esac
case "$config" in
    *'"engramind":{"name":"engramind-network"'*'"external":true'*) ;;
    *) fail 'the engramind network must be external (engramind-network by default)' ;;
esac
case "$config" in
    *'"s3":{"name":"minio-network"'*'"external":true'*) ;;
    *) fail 'the s3 network must be external (minio-network by default)' ;;
esac

# Variáveis obrigatórias precisam falhar a interpolação quando ausentes.
for name in APP_PORT CONTAINER_NAME TOKEN_PEPPER SECRETS_KEY MCP_ALLOWED_HOSTS; do
    if env -u "$name" docker compose -f docker-compose.yml config --quiet >/dev/null 2>&1; then
        fail "docker-compose.yml must require $name"
    fi
done

# Override de deploy: fixa a imagem do artefato e impede pull no servidor.
if docker compose -f docker-compose.yml -f docker-compose.deploy.yml config --quiet >/dev/null 2>&1; then
    fail 'docker-compose.deploy.yml must require API_IMAGE'
fi
deploy="$(API_IMAGE=engramind-api:validation docker compose -f docker-compose.yml -f docker-compose.deploy.yml config --format json | tr -d '[:space:]')"
case "$deploy" in
    *'"image":"engramind-api:validation"'*) ;;
    *) fail 'docker-compose.deploy.yml must use API_IMAGE' ;;
esac
case "$deploy" in
    *'"pull_policy":"never"'*) ;;
    *) fail 'docker-compose.deploy.yml must set pull_policy: never' ;;
esac

printf '%s\n' 'compose ok'
