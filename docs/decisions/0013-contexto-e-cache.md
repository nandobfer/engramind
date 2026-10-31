# 0013 — Contexto determinístico, `version` e cache

**Data**: 2026-10-30 · **Etapa**: 005 (0.5.0) · **Status**: aceita

## Contexto

O desenho (§8) pede um bloco de memórias estável para o cache do provedor de LLM, com `version`/ETag, `304` e um cache LRU em memória, sem tocar o banco quando nada mudou. Faltava decidir o corte das faixas, o significado do orçamento por canal, o que entra na `version`, a chave do cache e como evitar um bloco rotulado com uma versão que não é a dele.

## Decisão

- **Escolha independente do formato (Q-004).** O custo de cada memória é a linha `tagged` (`#id: texto` ou `#id: título`, já escapada), estimada por `context.token_estimator`; as tags não contam. `counts`, os ids e `injected_count` são os mesmos em `tagged`, `markdown` e `json`.
- **Corte (Q-001).** A primeira memória que não cabe no orçamento global fecha a faixa para todos; as seguintes vão para a faixa seguinte, mesmo menores. Quando as duas faixas fecham, o resto é excedente sem contar tokens.
- **Orçamento por canal (Q-002).** A sobreposição de `context.budget_tokens`/`context.titles_budget_tokens` no canal é o teto da parte dele em cada faixa: estourá-lo fecha a faixa só para aquele canal. O orçamento global continua valendo.
- **Agrupamento (Q-005).** Canais pela prioridade (desc) e slug; categorias pela prioridade (desc) e chave; memórias na ordem da escolha; `<titles>` na ordem da escolha.
- **`version` (D-001, Q-008).** `cv_` + 12 hex do SHA-256 sobre o formato resolvido, `(channel_id, channels.version, modo, prioridade)` dos canais alcançados (ordenados por id) e os valores de `context.budget_tokens`, `context.titles_budget_tokens`, `context.token_estimator`, `context.overflow_line` e `categories.list`. `context.default_format` fica fora: ele só escolhe o formato. As sobreposições por canal já mudam `channels.version`.
- **`304` sem banco (FR-009).** A `version` sai da rede em cache (`Core::network`, invalidada por toda mutação de canal, assinatura, agente e memória) e do `SettingsSnapshot` em memória.
- **Cache (D-003, Q-009).** LRU próprio de 1024 entradas, chave = digest completo de 32 bytes. Como o bloco depende só do que entra no hash, tokens com o mesmo alcance compartilham a entrada.
- **Instantâneo consistente (Q-010).** No cache miss, o `Store` lê as memórias e as versões dos canais numa mesma transação de leitura; a `version` devolvida e a chave do cache são recalculadas com essas versões.
- **Uso (Q-013, Q-014).** Só respostas com conteúdo somam `injected_count` (faixa 1) e gravam `usage_log`; o `304` não grava nada. `search_memories` grava os ids encontrados.

## Consequências

- O `304` custa só a resolução do alcance em memória.
- Um título trocado sem mudar `channels.version` deixaria o bloco antigo no cache: o Redator (006) precisa incrementar `channels.version` também quando aplica só o título.
- O cache é por processo; como há uma instância por banco, é o cache do serviço.
