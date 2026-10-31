# Clarify - Redator

**Atualizado em**: 2026-10-31

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Guarda de sentido sem embeddings | A guarda depende de embeddings, que chegam na 007. O que o Redator faz com o texto revisado nesta etapa? | resolvida | Falha fechada: a guarda é a interface `MeaningGuard`; em produção vale `NoGuard` (não mede) e só o título é aplicado. Os testes usam uma guarda determinística para cobrir o caminho do texto (revisão `system/redator`, `version + 1`, `embed`). A 007 liga a guarda real. |
| Q-002 | Preço das chamadas | `cost_usd` vem de onde? | resolvida | Só quando o provedor informa (`usage.cost` do OpenRouter); senão `NULL`. Tabela de preços fora até haver pedido. |
| Q-003 | Jobs `embed` sem handler | O que acontece com os `embed` até a 007? | resolvida | Cada runner só captura o próprio `kind`: os `embed` ficam `queued`; `get_status` mostra quantos. |
| Q-004 | O que o título altera | Aplicar só o título cria revisão e muda `memories.version`? | resolvida | Não. `title`, `title_state = ready`, `channels.version + 1`, `network_changed()` e auditoria `memory.redact` sem texto. Sem revisão e sem `memories.version + 1`, para não gerar `MEMORY_VERSION_CONFLICT` a quem edita. |
| Q-005 | Texto revisado duplicado | E se o texto revisado for igual ao de outra memória do canal? | resolvida | Mantém o original e aplica só o título. |
| Q-006 | Laço do Redator | A revisão do Redator reenfileira `redact`? | resolvida | Não: enfileira só `embed`. |
| Q-007 | Backlog sem Redator configurado | O FR-009 ao pé da letra marca o backlog como `fallback` quando a 0.6.0 sobe sem provedor. | resolvida | Literal (pedido da usuária: o serviço não é usado antes do fim das etapas). Desligado ou sem configuração → `fallback`, `done/skipped`. |
| Q-008 | Lease | Quanto dura o lease do `redact`? | resolvida | `timeout_ms × (1 + nº de fallback_models) + 60 s`. |
| Q-009 | Segredo em `update_settings` | O que acontece com `redator.api_key` em `update_settings`? | resolvida | `400 SETTING_IS_SECRET` (código novo), apontando `set_secret`. |
| Q-010 | `base_url` e chave por provedor | Quais são obrigatórias? | resolvida | Anthropic e Gemini: chave obrigatória, URL oficial como padrão (sobrescrevível). OpenAI-compatível: `base_url` obrigatória, chave opcional (servidores locais). |
| Q-011 | `test_redator` com proposta | A proposta aceita a chave? Grava algo? Idempotência? | resolvida | Aceita `redator.api_key` em claro, nunca gravada nem logada; grava só `llm_calls` (`purpose = test`). Aceita `Idempotency-Key` como toda operação não somente leitura (contrato §15); a resposta gravada não tem a chave. |
| Q-012 | Prompt | Onde fica e em que idioma? | resolvida | `src/core/redator/prompt.md`, embutido com `include_str!`; título e texto no idioma original. |
| Q-013 | Superado | Comparar `memories.version` com o payload? | resolvida | Não: fixar, arquivar e mover sobem a versão sem mudar o texto, e o título nunca chegaria. Superado = memória excluída ou `redact` mais novo para a mesma memória; na aplicação, o `content_hash` lido no início precisa bater (`MEMORY_CHANGED` → superado). |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | Um runner por tipo de job, com concorrência no código (`redact`: 2), acordado por `Notify` no commit e por intervalo de 1 s. | desenho §6 | Sem broker. |
| D-002 | Cliente LLM com três adaptadores: OpenAI-compatível (`/chat/completions` com `response_format: json_schema`, com JSON livre quando o servidor recusa o campo), Anthropic (`/v1/messages` com tool forçada) e Gemini (`generateContent` com `responseSchema`). | desenho §3, §12 | Testado contra servidor HTTP local roteirizado. |
| D-003 | `SECRETS_KEY` é a chave AES-256-GCM direto (32 bytes); formato gravado `v1 || nonce(12) || ciphertext`; o nome da chave é o dado associado. | desenho §12, §18 | Rotação de chave fora do escopo. |
| D-004 | Chaves de segredo válidas vêm do catálogo `SETTINGS` (`SettingType::Secret`): nesta etapa `redator.api_key`. | desenho §12 | `SECRET_UNKNOWN` para outras. |
| D-005 | `jobs.outcome` (coluna nova) com `applied`, `title_only`, `superseded` ou `skipped`. | FR-004 | Migração `0006`. |
| D-006 | Nova tag OpenAPI `Jobs` (admin) para `list_jobs`, `retry_job` e `list_llm_calls`; `set_secret` e `test_redator` ficam em `Settings`. | §13 | Guia `operacao` cobre `Jobs`. |

## Bloqueios

Nenhum.
