# Use sempre que desenvolver

## Antes de alterar

- Leia `AGENTS.md`, este arquivo, o contexto arquitetural, o guia específico da tarefa e a seção correspondente de `docs/design.md`.
- Confirme o que existe de fato no código, nos testes e nas migrações. O desenho descreve o alvo, não o estado.
- Valores de segredo podem ir para um `.env` local ignorado apenas quando o usuário autorizar; nunca os exiba, versione, documente ou registre.
- Separe fatos implementados, decisões tomadas e propostas abertas.

## Ao projetar uma mudança

- Respeite a etapa atual (`specs/NNN-etapa/`). Não antecipe capacidades de etapas seguintes sem pedido explícito.
- Coloque a regra no `core`; REST, MCP e CLI só traduzem entrada e saída.
- Toda operação nasce com nome base, parâmetros, escopo exigido e códigos de erro definidos, iguais nas três vias.
- Defina autenticação, escopo, acesso efetivo, validação, idempotência, auditoria e erro antes de tocar no banco.
- Toda mutação de memória grava revisão, auditoria, incrementa `memories.version` e `channels.version` e enfileira o webhook (§6).
- O contexto precisa permanecer determinístico: nada que não seja uma mudança real de memória, canal, assinatura ou configuração de renderização pode alterar a ordem ou o conteúdo.
- Tabela, coluna, dependência ou serviço fora do desenho exige necessidade concreta e decisão registrada.

## Segurança

- Nunca versione ou registre segredos, tokens ou chaves de API.
- Nunca inclua segredos em exemplos, fixtures, erros, build args ou imagens.
- Nunca registre o texto das memórias em `INFO` ou acima.
- Falhe fechado quando autenticação ou configuração obrigatória estiver ausente.
- A assinatura só restringe o acesso do token; nunca amplia.

## Qualidade

- Adicione teste para comportamento novo e regressão para bug corrigido.
- Testes usam SQLite temporário e provedores falsos determinísticos; nunca rede ou provedores reais.
- Execute primeiro as verificações focadas (`cargo test <filtro>`, Clippy, contrato). Reserve a suíte completa para pedido explícito ou mudanças amplas sem impacto delimitável.
- Confira core, REST, OpenAPI, MCP, CLI, guias e skill como um único contrato.
- Toda chamada externa tem timeout; retry só com backoff e para operações seguras.
- Provedores falsos nunca são fallback silencioso em produção.

## Documentação

- Atualize documentos que representam comportamento alterado, inclusive `docs/design.md` quando a implementação divergir.
- Registre decisões novas em `docs/decisions/`.
- Documente nomes de variáveis, nunca valores reais.
- Não documente comandos, arquivos, rotas ou configurações que não existam.

## Versionamento

- Toda alteração de código atualiza a versão em `Cargo.toml` (e `Cargo.lock`) e o `CHANGELOG.md` no mesmo trabalho.
- `PATCH` para correção ou ajuste interno; `MINOR` para nova capacidade ou comportamento público; `MAJOR` para quebra de contrato. Em `0.x`, quebra usa `MINOR`.
- OpenAPI e `get_status` derivam a versão de `CARGO_PKG_VERSION`; nunca a duplique em texto.
- Gere a tag `vX.Y.Z` apenas quando o usuário pedir uma release.
