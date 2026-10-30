# 0002 — `Store` e escritor único

**Data**: 2026-10-30 · **Etapa**: 001 (0.1.0) · **Status**: aceita

## Contexto

SQLite aceita um escritor por vez. O desenho (§16) pede uma thread escritora e um pool de leitores, e o `Store` como fronteira para uma futura implementação Postgres.

## Decisão

- `Store` é uma trait estática (sem `dyn`), com métodos `fn …(&self) -> impl Future<Output = …> + Send` (RPITIT): `async fn` em trait não garante futures `Send`, que os handlers axum genéricos em `S: Store` exigem. O `AppState` usa `type AppStore = SqliteStore`.
- **Escritor**: uma `std::thread` dona da única conexão de escrita, alimentada por `tokio::sync::mpsc` (capacidade 256) com closures e resposta por `oneshot`. `write` abre uma transação `IMMEDIATE` e confirma quando a closure termina sem erro. Pânico numa escrita vira `StoreError::Internal` e a thread continua.
- **Leitores**: `READ_POOL_SIZE` conexões `SQLITE_OPEN_READ_ONLY` (padrão 4), com `Semaphore` (espera máxima de 5 s → `Unavailable`) e `spawn_blocking`. Sem `r2d2`.
- PRAGMAs em todas as conexões: `journal_mode=WAL` (persistente, definido pelo escritor), `synchronous=NORMAL`, `busy_timeout=5000`, `foreign_keys=ON`.
- `write` e `read` são privados ao módulo `store::sqlite`; nenhum tipo do `rusqlite` aparece fora dele.
- Erros: `Busy`, `Locked`, `CannotOpen`, I/O, disco cheio e falta de memória → `Unavailable` (`503 DATABASE_UNAVAILABLE`); o resto → `Internal` (`500`).
- Testes usam o próprio `SqliteStore` em `tempdir`, nunca um falso.

## Consequências

- Trocar de banco altera só o alias e a implementação.
- Uma instância por banco continua sendo o limite (desenho §16).
