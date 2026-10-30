# 0001 — Dependências base

**Data**: 2026-10-30 · **Etapa**: 001 (0.1.0) · **Status**: aceita

## Contexto

O desenho (§3) fixa a stack; o sap-middleman é a referência para o que o desenho não decide.

## Decisão

| Crate | Uso |
|---|---|
| `axum` 0.8, `tokio` | HTTP e runtime |
| `rusqlite` 0.40 (`bundled`) | SQLite com FTS5 compilado |
| `rusqlite_migration` 2.6 | migrações embutidas (`migrations/*.sql`) |
| `sqlite-vec` 0.1.9 | extensão vetorial (registrada uma vez por processo) |
| `utoipa` 5 + `utoipa-swagger-ui` 9 (`vendored`, `debug-embed`) | OpenAPI e Swagger UI; `debug-embed` embute os arquivos da UI também em build de dev, sem o que os shards do CI (que não têm o `target/` da compilação) respondem 404 em `/docs/` |
| `clap` 4 | CLI (`engramind` / `eng`) |
| `tracing`, `tracing-subscriber` | logs `compact`/`json` |
| `thiserror`, `anyhow` | erros de domínio e de startup |
| `serde`, `serde_json`, `chrono`, `uuid` | serialização, timestamps, `x-request-id` |
| `dotenvy` | `.env` local |
| `zeroize`, `base64` | `Secret`, `SecretKey` (`SECRETS_KEY`) |
| `hmac`, `sha2` | verificador do `TOKEN_PEPPER` (e, na 002, o hash dos tokens) |

Dev: `http`, `http-body-util`, `tower`, `tempfile`.

## Consequências

- `rusqlite` fica em 0.40 porque `rusqlite_migration` 2.6 depende dela; subir para 0.41 exige uma versão nova do `rusqlite_migration`.
- `tower-http` (citado no desenho) não entrou: timeout, limite de corpo e request id são middlewares próprios de poucas linhas, como no sap-middleman, e devolvem o envelope de erro do Engramind. CORS fica fechado por ausência de camada.
- `utoipa` fica na major 5 (exigida pelo `AGENTS.md`), mesmo com a 6 disponível.
