# Clarify - consolidação

**Atualizado em**: 2026-10-30

## Perguntas

| ID | Tópico | Pergunta | Status | Resolução |
|----|--------|----------|--------|-----------|
| Q-001 | Escopo de `mark_verified` | Qual escopo e tipo de token? | aberta | Recomendação: any·write, sujeito ao acesso efetivo no canal. |
| Q-002 | `stale_candidate` no contexto | Memória sinalizada continua no contexto? | aberta | Recomendação: sim, sem mudança; é só sinal para a tela do gestor até ser arquivada. |

## Decisões

| ID | Decisão | Fonte | Impacto |
|----|---------|-------|---------|
| D-001 | A nota calculada vai para `importance_pending`; a próxima mutação real do canal promove `importance_pending → importance` na mesma transação que incrementa `channels.version`. Assim a ordem nunca muda sem mudança de `version`, inclusive após reinício. | desenho §2, §8 | Coluna nova na migração `0009`. |
| D-002 | Agendador com a crate `croner`; um job `consolidate` por canal elegível no horário. | desenho §10 | Nova dependência registrada. |

## Bloqueios

Q-001 e Q-002 antes da migração `0009`.
