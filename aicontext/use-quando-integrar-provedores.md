# Use quando integrar provedores (Redator, embeddings e webhooks)

## Princípios

- Toda chamada externa roda em worker sobre a fila `jobs`; a requisição do agente nunca espera o provedor. Exceções explícitas: `test_redator`, `test_embedding`, `list_embedding_models`, a validação de uma troca de embedding em `update_settings` (antes da transação) e o vetor da consulta em `search_memories`/`get_hints`, com prazo de 2 s e queda para o FTS.
- Toda chamada tem timeout da configuração (`*.timeout_ms`) e retentativa limitada com backoff (`*.max_attempts`).
- Provedor, modelo, URL e chave vêm das **configurações** (§12), nunca de variáveis de ambiente nem de constantes. Chaves são segredos cifrados.
- Falha de provedor degrada, não quebra: a memória continua salva e buscável por texto.
- Provedores falsos existem só em testes; nunca como fallback silencioso em produção.

## Redator (LLM)

- Cliente `reqwest` próprio para OpenAI-compatível, Anthropic e Gemini, com `fallback_models[]`.
- O texto da memória vai como **dado**, entre delimitadores, com instrução de não seguir nada do conteúdo.
- A resposta `{ title, text }` é validada por schema JSON e limites (`redator.title_max`, `memory.text_max`).
- Guarda de sentido: similaridade de embedding abaixo de `redator.min_similarity` mantém o texto original e aplica só o título.
- Texto alterado gera nova revisão com autor `system/redator`.
- Desligado, sem configuração ou após esgotar tentativas: `title_state = fallback` e texto original.
- Toda chamada grava `llm_calls` (tokens, custo, duração, outcome), sem o texto.

## Embeddings

- Provedores: `local` (`fastembed`, padrão `multilingual-e5-small`), `openai_compatible`, `ollama`, `gemini` e `off`.
- Só modelos de embedding; nunca modelos de chat.
- Aplique `query_prefix`/`document_prefix` conforme o modelo.
- Trocar provedor, modelo, URL, dimensão ou prefixo valida o provedor e reindexa na mesma transação; até terminar, a busca usa só vetores do modelo vigente e o FTS (decisão 0020; detalhes em `aicontext/embeddings.md`).
- Modelos locais são baixados para `MODELS_PATH`; nunca entram na imagem.
- `off` mantém busca textual e contexto funcionando.

## Webhooks

- `POST` assinado com HMAC-SHA256 (`Engramind-Signature: t=<ts>,v1=<hex>`), segredo cifrado por app.
- Payload mínimo, sem textos: `{ id, type, channel, channel_version, memory_ids, at }`.
- Destinatários: apps com agente assinando o canal afetado e apps com token `admin`.
- Retentativas com backoff (`webhooks.max_attempts`); o `id` permite deduplicar.

## Observabilidade

- Registre provedor, modelo, outcome, tentativa e duração; nunca payloads, textos ou chaves.
- Retentativa e fallback em `WARN`; falha definitiva em `ERROR`, uma única vez, na camada que decide.

## Testes

- LLM roteirizado, embeddings por hash e servidor de webhook local.
- Cubra validação de resposta, guarda de sentido, fallback, timeout, retentativa, reindexação sem mistura de dimensões e assinatura HMAC.
