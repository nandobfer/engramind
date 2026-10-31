# 0021 — Busca híbrida, dicas e guarda de sentido

**Data**: 2026-10-31 · **Etapa**: 007 (0.7.0) · **Status**: aceita

## Contexto

O desenho (§9) pede fusão RRF com pesos, `get_hints` com limiar alto só em `inject`/`search` e, no §7, a guarda de sentido do Redator por similaridade de embedding. A escrita não pode esperar o provedor.

## Decisão

- **RRF ponderado**: `score = Σ peso / (search.rrf_k + posição)` sobre o ramo vetorial e o textual, cada um com até o `limit` efetivo; 6 casas; desempate por `id`. `mode` (`hybrid`/`text`) descreve os ramos que rodaram (Q-012).
- **Consulta com prazo curto** (Q-019): `min(2 s, embedding.timeout_ms)`; passou, a busca segue só com o FTS, com `WARN embeddings.query_failed`. Sem configuração nova.
- **Dicas**: limiar sobre o cosseno (`1 - distância`) do ramo vetorial; só `Mode::Inject`/`Mode::Search`, mesmo com `manage` citado; exclui o texto completo do contexto; devolve `{ id, title, channel }`; sem embeddings, lista vazia (Q-001). Conta recuperação e `usage_log` (`op = get_hints`). Leitura por POST.
- **`similar` continua textual** na escrita (D-006): vetorizar o texto novo na requisição violaria o §7. O cosseno entre memórias vetorizadas é da 009.
- **Guarda de sentido**: `EmbeddingGuard` vetoriza original e revisado como documento numa chamada e devolve o cosseno; erro → `None` (falha fechada, só o título). `Core::set_meaning_guard` continua tendo precedência (testes).
- O título aplicado pelo Redator reenfileira `embed`.

## Consequências

- Os limiares padrão (`0.8`) são do desenho; com o e5, valores perto de `0.88` filtram melhor (documentado em `busca-e-dicas` e `configuracoes`).
