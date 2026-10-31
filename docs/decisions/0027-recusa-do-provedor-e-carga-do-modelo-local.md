# 0027 — Recusa do provedor, temperatura opcional e carga do modelo local fora da requisição

- Status: aceita (0.10.0)
- Contexto: desenho §7, §11 e §12; decisões 0017 e 0019.

## Contexto

Na 0.9.0 havia três problemas no uso real com provedores:

1. Os três adaptadores do LLM enviavam `temperature: 0` fixo. Modelos que recusam o campo (a
   família 5.5 da Anthropic responde `400 "temperature is deprecated for this model"`) não
   funcionavam no Redator nem no consolidador.
2. Toda recusa 4xx virava `LlmError::Rejected("HTTP 400")`: o motivo enviado pelo provedor era
   descartado, e `test_redator` respondia `502 PROVIDER_FAILED`. Atrás do Cloudflare, o corpo das
   respostas 5xx é substituído pela página do proxy, então o consumidor não via nem o envelope.
3. O primeiro uso do embedding local baixava o modelo dentro da requisição (o `multilingual-e5-large`
   leva ~67 s) e terminava em `504 REQUEST_TIMEOUT`, segurando o `Mutex` durante o download.

## Decisão

- **`redator.temperature`** (número de 0 a 2, padrão `null`, sem sobreposição por canal). `null`
  omite o campo do corpo nos três adaptadores; com valor, ele é enviado (`generationConfig.temperature`
  no Gemini). Vale para o Redator e para o consolidador, que compartilham `complete_with`; não há
  `consolidation.temperature`.
- **Motivo do provedor**: `provider_reason` extrai `error.message` (com `error.type` ou
  `error.status`) dos formatos OpenAI, Anthropic e Gemini, ou o corpo em texto; corpo HTML ou JSON sem
  mensagem não tem motivo. O texto vira uma linha, sem controles, com a chave configurada trocada por
  `[REDACTED]` e no máximo 160 caracteres; `Rejected` carrega `HTTP <status>: <motivo>`. Ele aparece em
  `details.reason`, na `message` e em `llm_calls.error`, e **nunca** em logs (pode repetir trecho do que
  foi enviado).
- **Código novo `PROVIDER_REJECTED` (422)** para recusas 4xx (exceto 408 e 429) do LLM e dos
  embeddings remotos. `PROVIDER_FAILED` (502) fica para indisponibilidade e resposta inválida e
  `PROVIDER_TIMEOUT` (504) para tempo limite. Um código continua tendo um único status.
- **Carga do modelo local fora da requisição**: o `LocalEmbedder` guarda o estado (`idle`,
  `downloading`, `loading`, `ready`, `failed`) e carrega cada modelo uma única vez numa thread própria,
  disparada na inicialização (`spawn_workers`), depois de uma troca de configuração para `local` e no
  primeiro uso. Quem chama espera até 5 s e depois recebe `EmbedError::Loading`, que vira
  `409 EMBEDDING_MODEL_LOADING` (com `model`, `state`, `started_at` e `elapsed_ms`) nas chamadas
  explícitas; o job `embed` volta à fila em 15 s **sem consumir tentativa** (`JobOutcome::Wait`); a busca
  cai para o FTS. Uma falha de carga é devolvida por 30 s (`PROVIDER_FAILED`, `outcome = load_failed`)
  e depois a próxima chamada tenta de novo. `get_status.embedding.local` mostra o estado.
- O carregador é injetável (`LocalEmbedder::with_loader`), para testar download, falha e espera sem
  rede nem relógio real.

## Consequências

- Mudança de contrato em `0.x`: recusas do provedor passam de 502 para 422 com outro código → versão
  MINOR (0.10.0).
- Status 409 (e não 503) para o modelo carregando, para o envelope atravessar proxies que trocam
  corpos 5xx.
- Fora do escopo: escolher a temperatura por modelo automaticamente e limitar o tempo total do
  `test_redator` com `fallback_models` ao `REQUEST_TIMEOUT_SECS`.
