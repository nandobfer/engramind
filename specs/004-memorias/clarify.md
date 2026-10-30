# Clarify - memórias

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Fila antes dos workers | Enfileirar `redact`/`embed` (e webhooks) antes de existirem workers? | resolvida | `redact` e `embed` desde já, com `{ memory_id, version }`; as memórias criadas antes da 006/007 serão processadas quando os workers chegarem. Webhooks só a partir da 008. |
| Q-002 | Duplicata na edição | A duplicata exata numa edição devolve a outra memória? | resolvida | Não: `409 MEMORY_DUPLICATE` com `details.existing_id`, sem alterar nada. Vale também para `revert` e para o destino do `move`. |
| Q-003 | Similaridade sem vetor | `memory.similar_threshold` é uma similaridade em [0,1], mas BM25 não é normalizado. | resolvida | Até a 007: 10 melhores candidatos do FTS no canal e nota = Jaccard dos termos normalizados; a partir da 007, cosseno quando houver vetor. O limiar é configurável (0 a 1). |
| Q-004 | Remover categoria em uso | `update_settings` pode retirar uma categoria usada por memórias? | resolvida | Não: `409 CATEGORY_IN_USE` com `details.in_use` (contagem por categoria). `label` e `priority` mudam livremente. |
| Q-005 | Memória fora do alcance | Operação por id numa memória de canal que o token não alcança? | resolvida | `404 MEMORY_NOT_FOUND` (não revela que existe), também fora do `Engramind-Channels`. Canal alcançado sem o escopo → `403 SCOPE_REQUIRED`. |
| Q-006 | Canal arquivado | Quais operações de memória um canal arquivado aceita? | resolvida | Leitura e `delete_memory`. Criar, editar, mudar status, reverter e mover (origem ou destino) → `409 CHANNEL_ARCHIVED`. |
| Q-007 | Edição parcial | O que grava uma edição sem mudança ou só de categoria? | resolvida | Sem mudança: `200` sem revisão nem versão. Só categoria: revisão e versões, sem zerar `importance` nem reenfileirar. Texto novo: tudo do FR-007. |
| Q-008 | `revert` e o Redator | O Redator reescreveria o texto revertido? | resolvida | `revert` restaura `text`, `title` e `category` da revisão e enfileira `embed` e `redact` com `title_only: true`. Conteúdo igual ao atual → `200` sem mudança. Colisão → `409 MEMORY_DUPLICATE`. |
| Q-009 | Duplicata com `Idempotency-Key` | A duplicata registra a chave? | resolvida | Sim, com status 200 e a resposta final; a repetição é exata. |
| Q-010 | `list_memories` | Propósito, padrões e ordem? | resolvida | `Purpose::Explicit` (arquivados só citados); status padrão `active,permanent`; ordem `updated_at DESC, id`; `since` sobre `updated_at`; `author` aceita `app`, `app/agente`, `app_…`, `ag_…` (inexistente → lista vazia); `fields` padrão `full`. |
| Q-011 | `search_memories` | Consulta, limites e resposta? | resolvida | Termos entre aspas unidos por `OR`, BM25 com peso 2 no título; limite 10 (máx. 50); resposta `{ results: [{ rank, score, memory }] }` com `score = 1/(60+rank)`; só a busca incrementa `retrieval_count`. |
| Q-012 | `channel_stats` | O que traz? | resolvida | `{ channel, channel_id, version, memories: { total, active, permanent, archived }, by_category, pending: { title, embedding }, retrievals, last_memory_at }`; exige `read` efetivo. |
| Q-013 | Configurações por canal | Onde ficam e como validam? | resolvida | `channels.settings_json`, só chaves com `channel_override` (hoje `memory.text_max`); outra chave → `400 SETTING_UNKNOWN` com `details.valid`; `null` remove; incrementa `channels.version`, audita e invalida o cache da rede, onde as sobreposições ficam. |
| Q-014 | Validação de `categories.list` | Formato? | resolvida | `key` `^[A-Z][A-Z0-9_]{0,39}$` única, `label` de 1 a 80, `priority` de -1000 a 1000, de 1 a 50 itens; a primeira é a padrão. |
| Q-015 | `delete_channel` | Apaga as memórias? | resolvida | Sim, com as revisões delas; a auditoria registra `memories_removed`. |
| Q-016 | Entrada na CLI | Como passar texto longo? | resolvida | `--text` obrigatório em `create`; `--text -` lê da entrada padrão (erro local `CLI_INPUT_INVALID`); `--origin-source`/`--origin-label`, `--expected-version`. |
| Q-017 | Autoria na resposta | Como mostrar o autor? | resolvida | `author: { app_id, app, agent_id, agent }`; slugs por LEFT JOIN, `null` quando o app ou o agente foi excluído. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | `content_hash` = SHA-256 do texto em NFC, sem espaços nas pontas, com espaços internos colapsados e minúsculo. | desenho §7 | Nova dependência `unicode-normalization`. |
| D-002 | `memory_revisions.memory_id` sem chave estrangeira, para a última revisão sobreviver ao `delete`. | desenho §6 | Limpeza de revisões órfãs não existe; é intencional. |
| D-003 | Autoria em `author_app_id`/`author_agent_id` sem chave estrangeira (ver 002 D-006). Revisões do Redator usam `app_id = 'system'`, `agent_id = 'redator'`. | desenho §7 | — |
| D-004 | Contadores acumulados em `Mutex<HashMap>` e gravados a cada 5 s pelo escritor, sem mudar `version` nem a ordem do contexto. | desenho §6, §8 | — |
| D-005 | Status iniciais aceitos em `create_memory`: `active` (padrão) e `permanent`. | desenho §7 | — |
| D-006 | `memories.seq INTEGER PRIMARY KEY` é o rowid do FTS externo; `id TEXT UNIQUE` é o id público. | rowid implícito muda no `VACUUM` | Diverge do `id TEXT PK` previsto no `contracts.md`; registrado no desenho §6. |
| D-007 | Mutações recebem `MemoryTarget { id, channel_id }`: memória que mudou de canal entre o acesso e a escrita → `MEMORY_NOT_FOUND`. | Q-005 | Fecha a corrida com `move`. |
| D-008 | O `core` calcula `similar` antes da escrita e o `Store` o inclui na resposta, para a idempotência guardar a resposta final. | idempotência na mesma transação | `NewMemory`/`MemoryPatch` levam `similar`. |
| D-009 | Toda mutação de memória chama `network_changed()`, porque muda `channels.version` (exibida por `get_channel`). | cache da 003 | Uma releitura da rede por escrita. |
| D-010 | Arquivos planos no `core` (`memories.rs`, `memory_text.rs`, `search.rs`, `counters.rs`), como na 002 e na 003. | layout | Diverge dos diretórios previstos no `plan.md`. |
| D-011 | `search_memories` é leitura por POST: `read_only`, sem `Idempotency-Key`. O teste do catálogo aceita essa exceção. | corpo com filtros | — |
| D-012 | A duplicata considera memórias de qualquer status do canal. | simplicidade | Uma arquivada igual é devolvida com `duplicate: true` e pode ser restaurada. |
| D-013 | `doctor` roda `integrity-check` do `memory_fts` pela conexão escritora. | risco do trigger | Novo item `memory_fts` no `doctor`. |

## Bloqueios

Nenhum.
