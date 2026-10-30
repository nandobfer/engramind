# Clarify - integração

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Segredo do webhook | Quem gera o segredo do webhook do app? | aberta | Recomendação: `update_app { webhook_url, webhook_secret }`, com o segredo só de escrita (cifrado em `apps.webhook_secret_enc`) e a leitura `{ configured, updated_at }`, igual aos segredos de configuração. |
| Q-002 | Proteção do `/metrics` | Token `admin·read` ou rede interna? | aberta | Recomendação: token `admin·read` sempre (D-002); sem variável para abrir. |
| Q-003 | Autor na importação | O autor preservado precisa existir como app/agente? | aberta | Recomendação: sim, `author: { app, agent? }` precisa existir (`APP_NOT_FOUND`/`AGENT_NOT_FOUND`); quem importa de outro sistema cria antes um app que representa a origem. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | Evento = um job `webhook` por destinatário, criado na transação da mutação; o payload é o mesmo e o `id` é o do evento. | desenho §6, §17 | Entrega independente por app. |
| D-002 | `/metrics` atrás do `require_auth` com exigência `admin·read`; Prometheus usa `authorization.credentials`. | desenho §20 | Fora de `/v1`, sem OpenAPI de domínio (rota técnica documentada). |
| D-003 | Retenção roda como job agendado diário na própria fila. | desenho §6 | — |

## Bloqueios

Q-001 a Q-003 antes da migração `0008`.
