# 0014 — Estimativa de tokens com `tiktoken-rs`

**Data**: 2026-10-30 · **Etapa**: 005 (0.5.0) · **Status**: aceita

## Contexto

As faixas do contexto (§8) e `rendered_tokens` precisam de uma contagem de tokens próxima da dos provedores de LLM, determinística e sem rede. O desenho (§12) prevê `context.token_estimator` com `o200k` por padrão.

## Decisão

- Dependência `tiktoken-rs` 0.12, só as codificações `o200k_base` (padrão) e `cl100k_base`, atrás do trait `TokenEstimator` (`core::context::estimator`).
- As tabelas BPE são carregadas no primeiro uso (singletons do crate). `engramind serve` aquece a codificação configurada com `spawn_blocking` logo depois de subir; os testes só pagam o custo quando renderizam (regra das inicializações preguiçosas da 001).
- A renderização roda em `spawn_blocking`, porque contar milhares de linhas é trabalho de CPU.
- Os unitários usam um estimador determinístico de palavras; a integração usa o real.

## Consequências

- O binário cresce alguns MB com as tabelas embutidas; nada é baixado em tempo de execução.
- A contagem é uma estimativa: provedores com outra codificação contam um pouco diferente. Por isso os orçamentos têm folga e `rendered_tokens` é informativo.
