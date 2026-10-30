# 0006 — CD por artefato (skill `setup-ci-cd`)

**Data**: 2026-10-30 · **Etapa**: 001 (0.1.0) · **Status**: aceita

## Decisão

- Na `main`, depois de `Lint`, `Test (i/4)` e `Compose`: a imagem é construída no runner (Buildx, cache `gha` da camada `cargo-chef`), passa por um smoke test (`/health/ready` e `eng doctor` com segredos efêmeros), é exportada com `docker save | gzip -9`, ganha `sha256sum` e vira artefato por 3 dias.
- `Deploy production` (Environment `production`, `concurrency: deploy-production` sem cancelamento) roda `scripts/deploy-production.ts`, copiado do `opex`: SSH com `known_hosts` fixo, SCP da imagem e dos dois arquivos Compose, checksum, `docker load` e `docker compose up -d --no-build --pull never api` com `docker-compose.deploy.yml` (`API_IMAGE`, `pull_policy: never`). Depois, confere a imagem do contêiner e o healthcheck, e indica a imagem anterior para rollback.
- Adaptações em relação ao `opex`: `OPEX_IMAGE` → `API_IMAGE`; os padrões passam a ser `api`, `engramind-api` e `engramind-network,minio-network`; as referências de documentação foram trocadas.
- O servidor não faz build nem pull. A rede `engramind-network`, o diretório e o `.env` são preparados uma vez no servidor.
- Uma instância por banco: o Compose para o contêiner antigo e sobe o novo. Rollback de imagem só é seguro sem migração nova; com migração nova, o rollback é restaurar do Litestream. Migrações são sempre aditivas.
