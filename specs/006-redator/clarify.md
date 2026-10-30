# Clarify - Redator

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Guarda de sentido sem embeddings | A guarda depende de embeddings, que chegam na 007. O que o Redator faz com o texto revisado nesta etapa? | aberta | Recomendação: falhar fechado. Sem provedor de embeddings ativo, a guarda não pode ser verificada e só o título é aplicado; a revisão de texto passa a valer na 007. Alternativa: inverter a ordem das etapas 006 e 007. |
| Q-002 | Preço das chamadas | `cost_usd` vem de onde? | aberta | Recomendação: só quando o provedor informa (ex.: `usage.cost` do OpenRouter); caso contrário `NULL`. Tabela de preços configurável fica fora até haver pedido. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | Um `JobRunner` por tipo de job, cada um com concorrência configurada no código (`redact`: 2), acordado por `Notify` no commit e por intervalo de 1 s. | desenho §6 | Sem broker. |
| D-002 | Cliente LLM com três adaptadores: OpenAI-compatível (`/chat/completions` com `response_format: json_schema` quando suportado, senão JSON livre validado), Anthropic (`/v1/messages` com tool forçada para saída estruturada) e Gemini (`generateContent` com `responseSchema`). | desenho §3, §12 | Testado contra servidor HTTP local roteirizado. |
| D-003 | `SECRETS_KEY` deriva a chave AES-256-GCM direto (32 bytes); formato gravado `v1 || nonce(12) || ciphertext`. | desenho §12, §18 | Rotação de chave fora do escopo. |
| D-004 | Chaves de segredo válidas vêm do catálogo `SETTINGS` (campos marcados como segredo): nesta etapa `redator.api_key`. | desenho §12 | `SECRET_UNKNOWN` para outras. |

## Bloqueios

Q-001 antes do passo de aplicação do texto revisado.
