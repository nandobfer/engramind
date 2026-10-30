# Use para entender o propósito e a arquitetura

## Propósito

O Haikocampus centraliza as memórias de agentes de IA, que hoje ficam espalhadas por sistema e por papel, e deixa qualquer sistema consumir a mesma rede. Guarda fatos duráveis e curtos em canais, redige título e texto pelo Redator e entrega o contexto pronto para o prompt, estável para o cache do provedor de LLM.

Glossário completo em §0 do desenho. Termos centrais: **app**, **agente**, **agente curador**, **canal**, **assinatura** (modo `inject`/`search`/`manage` e acesso), **Redator** e **contexto**.

## Fluxo de confiança

```text
Sistema consumidor / gestor / agente com shell
  -> Bearer hk.<token_id>.<segredo>
Haikocampus
  -> Principal { app, agent?, kind, scopes }
  -> acesso efetivo = escopos do token ∩ acesso da assinatura
  -> core (mesmas regras para REST, MCP e CLI)
  -> Store (SQLite) e provedores configurados (LLM, embeddings, webhooks)
```

## Responsabilidades do serviço

- Autenticar e identificar app e agente; aplicar escopos e acesso efetivo.
- Guardar memórias com proveniência, revisões, auditoria e reversão.
- Redigir título e texto (Redator) e gerar embeddings, de forma assíncrona.
- Renderizar o contexto determinístico com orçamento em duas faixas, `version`/ETag e `304`.
- Buscar (FTS5 + sqlite-vec com RRF) e fornecer dicas curtas.
- Expor configurações, identidades e operação por REST, MCP e CLI.
- Notificar mudanças por webhook assinado.
- Servir a documentação da versão em execução.

## Responsabilidades dos consumidores

- Decidir a política fina: quais ferramentas o agente vê e quando ele escreve.
- Extrair memórias de conversas, se quiserem (ex.: agente curador).
- Colocar o contexto na parte estável do prompt e as dicas no fim do turno.
- Reagir ao webhook ou ao `304` para reconstruir o prompt.
- Tratar códigos de erro estáveis.

## Arquitetura alvo

- Binário Rust único: servidor (`haikocampus serve`) e CLI (`haiko`).
- `axum` 0.8 + `tokio` + `tower-http` (timeout, limite de corpo, tracing, request id).
- `core` único; adaptadores REST (`utoipa`), MCP (`rmcp`, stateless) e CLI (`clap`, `--json`).
- `Store` com implementação SQLite: `rusqlite` bundled, WAL, FTS5, `sqlite-vec`; uma thread escritora e um pool de leitores.
- Workers `tokio` sobre a fila `jobs` em tabela: Redator, embeddings, consolidador e webhooks.
- Embeddings locais (`fastembed`, `multilingual-e5-small`) por padrão, ou remotos por configuração.
- Litestream na imagem replicando para S3.
- Uma instância por banco; escalar horizontalmente exige a implementação Postgres + pgvector do `Store` (fase 5).

## Fases

1. Núcleo — identidades, canais, assinaturas, memórias, contexto, FTS5, REST/OpenAPI, CLI, Litestream, contrato.
2. Inteligência — jobs, Redator, embeddings, busca híbrida, dicas, configurações, MCP completo e skill.
3. Integração — webhooks, importação, métricas, guias de integração.
4. Consolidação — duplicatas, importância, desuso, `mark_verified`.
5. Escala, se necessário.

Detalhes em [`../docs/design.md`](../docs/design.md).
