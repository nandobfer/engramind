# Use quando configurar deploy e reverse proxy

## Ambiente

- Variáveis de ambiente cuidam só de infraestrutura: porta, caminho do banco e dos modelos, pepper, chave de cifra, timeouts, limites, hosts do MCP, Litestream e logs. Lista alvo em §16 do desenho (`.env.example`).
- Comportamento funcional (Redator, embeddings, orçamentos) nunca vira variável de ambiente.
- Variáveis obrigatórias (`APP_PORT`, `TOKEN_PEPPER`, `SECRETS_KEY`) impedem o startup quando ausentes ou inválidas.
- `.env` nunca é versionado; `.env.example` só com nomes e valores de exemplo.

## Docker

- `Dockerfile` multi-stage só de produção: `chef` (`rust:1.98.0-bookworm` + `cargo-chef` fixo) → `planner` → `builder` (camada de dependências cacheada) → `runtime` (`debian:bookworm-slim`, usuário `engramind` sem privilégios, `/data` com o dono certo, link `eng`, Litestream de versão fixa, entrypoint, `HEALTHCHECK` em `/health/live`). O ONNX Runtime entra na 007.
- Desenvolvimento e testes rodam fora do Docker (`cargo run -- serve`, `cargo nextest run`).
- `docker-compose.yml` com um único serviço `api`, sem perfis: `name: engramind-${APP_PORT}`, `image: ${API_IMAGE:-engramind-api:latest}`, `container_name: ${CONTAINER_NAME}`, `init: true`, `restart: unless-stopped`, limites de CPU e memória, redes externas `engramind-network` (`ENGRAMIND_NETWORK`, consumidores alcançam `http://${CONTAINER_NAME}:${APP_PORT}`) e `minio-network` (`S3_NETWORK`, Litestream) e volume nomeado `engramind-data:/data`. Antes do primeiro `up`: `docker network create engramind-network`.
- Porta padrão `8000`, registrada em `/home/used_ports`.
- `./scripts/check-compose.sh` valida o compose e o override de deploy (também no CI).
- `name` derivado de `APP_PORT` evita que stacks de devs diferentes no mesmo host disputem o mesmo projeto Compose.

## Litestream

- Binário copiado de imagem com versão fixa.
- `docker/entrypoint.sh`: `litestream restore -if-db-not-exists -if-replica-exists` e depois `exec litestream replicate -exec "engramind serve"`. Com argumentos, executa o comando informado (`docker compose exec api eng doctor`).
- `litestream.yml` (v0.5, chave `replica:` no singular) replica para o MinIO do host: bucket `ENGRAMIND_S3_BUCKET`, caminho `ENGRAMIND_S3_PATH`, endpoint `ENGRAMIND_S3_ENDPOINT`.
- `engramind doctor` lê a idade da última réplica com `litestream ltx -json -level all`; sem o binário, o item é pulado.
- `LITESTREAM_ENABLED=false` roda `engramind serve` direto; `get_status` avisa que não há backup.
- Credenciais S3 só por variável de ambiente.
- Uma instância por banco: o deploy recria o contêiner com parada curta. Nunca suba duas instâncias apontando para o mesmo volume.

## CD

- Workflow `.github/workflows/ci-cd.yml`: `Lint`, `Build tests`, `Test (i/4)`, `Compose` em todo PR e push; na `main`, `Coverage`, `Build image` (smoke test) e `Deploy production`.
- `scripts/deploy-production.ts` (copiado do `opex`) transfere a imagem por SCP, faz `docker load` e `docker compose up -d --no-build --pull never api` com `docker-compose.deploy.yml` (`API_IMAGE`).
- Environment `production`: Secrets `DEPLOY_SSH_KEY`, `DEPLOY_KNOWN_HOSTS`, `DEPLOY_HOST`, `DEPLOY_SSH_USER`; Variables `DEPLOY_REMOTE_DIR`, `DEPLOY_PROJECT_NAME`, `DEPLOY_HEALTHCHECK_URL` e os opcionais `DEPLOY_SSH_PORT`, `DEPLOY_SERVICE`, `DEPLOY_IMAGE_REPOSITORY`, `DEPLOY_PLATFORM`, `DEPLOY_REQUIRED_NETWORKS`.
- O servidor precisa de `DEPLOY_REMOTE_DIR/.env` e das redes externas; o pipeline não os cria.
- Rollback de imagem só sem migração nova; com migração nova, restaure do Litestream.

## Reverse proxy

- O proxy pode publicar sob qualquer prefixo e deve removê-lo ao encaminhar (`location /prefixo/ { proxy_pass http://host:porta/; }`).
- Nunca codifique o prefixo no binário.
- A raiz e `/docs` redirecionam de forma relativa para `docs/`; o Swagger carrega `../openapi.json`; o OpenAPI declara `servers: [{ url: "." }]`.
- `MCP_ALLOWED_HOSTS` inclui o hostname público entregue pelo proxy.

## Verificação focada

- `/health/live` e `/health/ready` respondem; `engramind doctor` confere integridade, migrações, extensões e idade da réplica.
- Raiz e `/docs` sem seguir redirect retornam `Location: docs/`.
- `POST <prefixo>/mcp` com Bearer e `initialize` funciona pelo prefixo público.
- Não rode a suíte completa só por uma mudança de deploy.
