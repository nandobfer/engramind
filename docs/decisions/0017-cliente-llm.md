# 0017 — Cliente LLM próprio

**Data**: 2026-10-31 · **Etapa**: 006 (0.6.0) · **Status**: aceita

## Contexto

O Redator chama três famílias de provedor (desenho §3 e §12) e precisa de saída JSON estruturada, tempo limite por chamada, troca de modelo em falha e um registro de cada chamada sem o texto.

## Decisão

- **Sem SDK (D-002).** `src/providers/llm/` usa o `reqwest` que já existia (rustls, sem redirecionamento). A trait `LlmClient::complete_json(LlmRequest)` tem três adaptadores:
  - OpenAI-compatível: `POST {base_url}/chat/completions` com `response_format: json_schema` (estrito); um `400`/`422` que cita `response_format` repete o pedido sem ele, em JSON livre;
  - Anthropic: `POST {base_url}/v1/messages` com `anthropic-version: 2023-06-01` e uma tool forçada (`tool_choice`), cujo `input_schema` é o esquema da saída;
  - Gemini: `POST {base_url}/v1beta/models/{model}:generateContent` com `responseMimeType: application/json` e `responseSchema` (tipos em maiúsculas, sem `additionalProperties`).
- **Erros.** `Timeout` (tempo limite, `408`), `RateLimited` (`429`), `Unavailable` (`5xx` ou conexão), `Rejected` (outro `4xx`) e `InvalidResponse`. Só os três primeiros passam para o próximo de `redator.fallback_models`. Nenhum erro carrega o corpo do provedor, que pode ecoar o texto enviado.
- **Validação no serviço.** O objeto devolvido é validado de novo (`title` de uma linha até `redator.title_max`, `text` em `memory.text_min`/`text_max`, sem campos extras), independente do modo do provedor; inválido conta como falha da tentativa.
- **`llm_calls` (FR-011, Q-002).** Uma linha por chamada com finalidade, provedor, modelo, tokens, custo, duração, resultado e erro curto. `cost_usd` só quando o provedor informa (`usage.cost` do OpenRouter); senão `NULL`. Tabela de preços configurável fica fora até haver pedido.
- **Prompt (FR-005).** Prompt do sistema em `src/core/redator/prompt.md` (embutido), com `redator.instructions` no fim; o texto vai na mensagem do usuário entre `<<<MEMORIA-x>>>` e `<<<FIM-MEMORIA-x>>>`, com `x` aleatório por chamada, que o conteúdo não consegue prever nem fechar.

## Consequências

- Os testes usam um servidor HTTP local roteirizado (`tests/it/support/llm.rs`), sem rede.
- Provedores novos são um adaptador e um valor em `redator.provider`.
