# 0029 — Foco no contexto

- Status: aceita (0.11.0)
- Contexto: desenho §8 e §9; decisões 0011 (ordem determinística), 0021 (busca híbrida).

## Contexto

O bloco de `get_context` é estável e ordenado só por prioridade, status, categoria e importância.
Com muitos canais, o que importa para a tarefa do turno pode cair para a faixa de títulos.

## Decisão

- `get_context` aceita `focus` (REST, MCP e CLI; o resource continua sem foco), normalizado
  (`trim` e espaços juntados; vazio = sem foco) e limitado a 2000 caracteres (`400 FOCUS_TOO_LONG`).
- Sem LLM: é **relevante** a memória com cosseno ≥ `context.focus_min_score` no ramo vetorial ou com
  algum termo casado no FTS5 (até 50 por ramo, nos canais do plano). O score da fusão RRF
  (`Σ peso/(k+posição)`) não serve de limiar, então o limiar vale só para o cosseno e a fusão RRF
  apenas ordena as relevantes.
- Na ordem, as relevantes vêm depois da prioridade do canal e antes de todo o resto: o foco reordena
  dentro do canal, nunca troca a regra de prioridade nem os orçamentos.
- A `version` com foco inclui o SHA-256 do foco normalizado (nunca o texto), o `model_id` vigente,
  o limiar e o modo esperado (`hybrid` com embeddings, `text` sem). Sem foco, nada muda.
  `context.focus_min_score` e `context.focus_embed_timeout_ms` ficam fora de `CONTEXT_RENDER_KEYS`.
- Degradação: embedding com falha ou fora do prazo → `focus_mode: text`; FTS com falha →
  `focus_mode: none` (bloco sem foco). O bloco degradado recebe outra `version` e não entra em cache.
- Os blocos com foco vão para um LRU próprio de 64 entradas.
- O foco nunca é gravado: só `focus_present`, `focus_len` e `focus_mode` num evento `DEBUG`.

## Consequências

- Cada turno com foco paga um embedding (até o prazo) e a varredura exata (decisão 0018).
- Um embedding que chega depois sem mudar `channels.version` não muda a `version` do foco: o `304`
  pode manter a relevância antiga até a próxima mudança real. Limitação aceita e documentada.
