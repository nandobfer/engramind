# 0025 — Agendador cron da consolidação

**Data**: 2026-10-31 · **Etapa**: 009 (0.9.0) · **Status**: aceita

## Contexto

O desenho §10/§12 agenda a consolidação por uma expressão cron (`consolidation.schedule`, padrão `0 3 * * *`). A fila `jobs` não tem agendamento, e as tarefas periódicas do servidor vivem em `src/core/background.rs`.

## Decisão

- Dependência `croner` 4.0.2 (recursos padrão, `chrono`): parser com `Seconds::Disallowed` e `Year::Disallowed`, só expressões de 5 campos (apelidos como `@daily` são recusados). A expressão é avaliada em UTC.
- Tipo de configuração novo `SettingType::Cron`, validado por `consolidation::schedule::parse` no `update_settings`.
- `Core::consolidation_tick(now)` roda a cada 60 s em `background.rs`. O último horário disparado fica em `schema_meta['consolidation.last_scheduled']`; se houver algum horário em `(último, agora]`, enfileira um `consolidate` por canal habilitado sem job pendente e grava `agora` na mesma transação. Vários horários perdidos (serviço desligado) disparam uma vez só. Sem registro (primeira subida), só grava a base, sem disparar.

## Consequências

- Uma instância por banco (limite do SQLite) garante um disparo por horário sem coordenação.
- Trocar a expressão vale a partir do último horário registrado: uma expressão nova com horário já passado dispara uma vez no minuto seguinte.
