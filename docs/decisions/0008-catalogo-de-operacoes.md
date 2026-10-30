# 0008 — Catálogo único de operações

**Data**: 2026-10-30 · **Etapa**: 002 (0.2.0) · **Status**: aceita

## Contexto

Cada operação precisa existir nas três vias (REST, MCP e CLI) com o mesmo nome, parâmetros, escopo e erros (§13), e os testes de contrato (§15) precisam pegar qualquer divergência.

## Decisão

- `core::operations::OPERATIONS` é a fonte única de cada operação: nome base (= `operationId` = ferramenta MCP), tag OpenAPI, resumo, tipo exigido (`Any`/`Admin`), escopo, hints MCP (`read_only`, `destructive`, `idempotent`), método e caminho REST e caminho da CLI.
- `authz::require` lê o catálogo e responde `403 SCOPE_REQUIRED` com `details: { required_kind, required_scope }`. Toda operação do `core` começa por `Core::authorize`, que também grava o nome no campo `operation` do span.
- `operationId` é o nome base em snake_case (`issue_token`), diferente do camelCase do sap-middleman.
- Os testes de contrato cruzam o catálogo com o OpenAPI (método, caminho, tag, resumo, segurança, `Idempotency-Key` nas mutações), com as ferramentas MCP (nomes, hints e o mesmo conjunto de parâmetros), com `engramind help --json` e com os guias (cada operação em exatamente um guia, pela tag).
- Tags administrativas (`Identities`, `Settings`, `Status`, `Audit`) contêm exatamente as operações `Admin`; elas, os guias `admin` e os esquemas que só elas usam ficam ocultos para tokens `agent` na documentação.
- Mutações: idempotência genérica no `core` (`Core::mutate`). A repetição é conferida antes de resolver os alvos, então repetir uma exclusão devolve a resposta gravada, não `404`. A reserva da chave e a resposta são gravadas na mesma transação da mutação e da auditoria. `issue_token` repetido devolve o token **sem** `secret`, porque o segredo nunca é persistido.
- `LAST_ADMIN_TOKEN` é conferido dentro da transação de toda mudança que pode desativar um token admin com `write` (revogar, alterar escopos ou expiração, desativar ou excluir app e agente): se havia algum antes e não sobra nenhum depois, a transação é desfeita.

## Consequências

- Uma operação nova exige a entrada no catálogo, o handler anotado, a ferramenta MCP, o comando da CLI e o guia; sem isso, os testes de contrato falham.
