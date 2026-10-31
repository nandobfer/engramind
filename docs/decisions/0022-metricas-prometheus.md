# 0022 — Métricas Prometheus

**Data**: 2026-10-31 · **Etapa**: 008 (0.8.0) · **Status**: aceita

## Contexto

O desenho (§20) pede `/metrics` com latência por operação, taxa de `304`, fila, custo do Redator, duração dos embeddings e idade da réplica. O serviço de referência (sap-middleman) já usa `metrics` + `metrics-exporter-prometheus` com o recorder instalado uma vez. Todo o MCP passa por uma rota só (`/mcp`), então a rota não identifica a operação.

## Decisão

- Dependências `metrics` 0.24 e `metrics-exporter-prometheus` 0.18.3 (sem features padrão); `src/metrics.rs` instala o recorder uma vez (`OnceLock`) e expõe `GET /metrics` (formato de texto 0.0.4).
- `/metrics` exige sempre token `admin` com `read` (Q-002): fica fora de `/v1` e do catálogo de operações (não é ferramenta MCP nem comando da CLI), documentado no OpenAPI como `get_metrics` na tag `Operations`. Diverge do sap-middleman, onde é público.
- Métricas HTTP com os nomes do sap-middleman (`http_requests_total`, `http_request_duration_seconds`) e o rótulo `operation`: o middleware de log cria um `OperationSlot` por requisição, preenchido pela rota REST do catálogo ou pela ferramenta MCP chamada (a extensão viaja nos `Parts` que o `rmcp` repassa à ferramenta). A taxa de `304` sai de `http_requests_total{operation="get_context",status="304"}`.
- Métricas de domínio com prefixo `engramind_`: fila (gauge por tipo e estado, lido no scrape), falhas definitivas, chamadas, tokens e custo do LLM (em micro-dólares, porque os contadores são inteiros), duração e falhas dos embeddings, entregas de webhook, falhas de autenticação (`missing`/`invalid`/`inactive`), tamanho do banco, idade da réplica, retenção, prontidão e versão.
- Rótulos fechados: nunca slug, id, URL ou texto.

## Consequências

- Os contadores são do processo e zeram no reinício (o Prometheus trata com `rate`).
- O scrape faz duas leituras baratas (contagem da fila e `page_count`); a réplica vem do amostrador de 60 s (Q-009), nunca de um `litestream ltx` por scrape.
