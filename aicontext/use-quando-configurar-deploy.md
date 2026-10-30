# Use quando configurar deploy e reverse proxy

## Ambiente

- Variáveis de ambiente cuidam só de infraestrutura: porta, caminho do banco e dos modelos, pepper, chave de cifra, timeouts, limites, hosts do MCP, Litestream e logs. Lista alvo em §16 do desenho (`.env.example`).
- Comportamento funcional (Redator, embeddings, orçamentos) nunca vira variável de ambiente.
- Variáveis obrigatórias (`APP_PORT`, `TOKEN_PEPPER`, `SECRETS_KEY`) impedem o startup quando ausentes ou inválidas.
- `.env` nunca é versionado; `.env.example` só com nomes e valores de exemplo.

## Docker

- `Dockerfile` multi-stage só de produção: `builder` (`rust:<versão>-bookworm`, `cargo build --locked --release` com cache) e `runtime` (`debian:bookworm-slim`, usuário sem privilégios, ONNX Runtime, Litestream, entrypoint, `HEALTHCHECK` em `/health/live`).
- Desenvolvimento e testes rodam fora do Docker (`cargo run -- serve`, `cargo test`).
- `docker-compose.yml` com um único serviço `api`, sem perfis: `name: engramind-${APP_PORT}`, `image: ${API_IMAGE:-engramind-api:latest}`, `container_name: ${CONTAINER_NAME}`, `init: true`, `restart: unless-stopped`, limites de CPU e memória, rede externa configurável e volume nomeado `engramind-data:/data`.
- `name` derivado de `APP_PORT` evita que stacks de devs diferentes no mesmo host disputem o mesmo projeto Compose.

## Litestream

- Binário copiado de imagem com versão fixa.
- `docker/entrypoint.sh`: `litestream restore -if-db-not-exists -if-replica-exists` e depois `exec litestream replicate -exec "engramind serve"`.
- `LITESTREAM_ENABLED=false` roda `engramind serve` direto; `get_status` avisa que não há backup.
- Credenciais S3 só por variável de ambiente.
- Uma instância por banco: o deploy recria o contêiner com parada curta. Nunca suba duas instâncias apontando para o mesmo volume.

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
