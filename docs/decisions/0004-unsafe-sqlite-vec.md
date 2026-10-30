# 0004 — Único bloco `unsafe`: registro do `sqlite-vec`

**Data**: 2026-10-30 · **Etapa**: 001 (0.1.0) · **Status**: aceita

## Contexto

O sap-middleman usa `unsafe_code = "forbid"`. O `sqlite-vec` só é carregado por `sqlite3_auto_extension`, uma chamada FFI.

## Decisão

- `unsafe_code = "deny"` no crate.
- Um único bloco `unsafe`, em `src/store/sqlite/vec_extension.rs`, com `#![allow(unsafe_code)]` no módulo e comentário `SAFETY`: o `transmute` só restaura a assinatura real de `sqlite3_vec_init` (a crate a declara sem parâmetros), como no exemplo oficial da crate.
- O registro acontece uma vez por processo (`OnceLock`) antes da primeira conexão; toda conexão aberta depois já tem `vec_version()` e `vec0`.

## Consequências

Qualquer outro `unsafe` falha a compilação. O teste `store::sqlite::tests::sqlite_vec_and_fts5_are_available` garante o carregamento.
