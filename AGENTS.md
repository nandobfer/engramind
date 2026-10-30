# Engramind — Instruções para agentes

Este arquivo é normativo para qualquer agente que trabalhe neste repositório.

## Leitura obrigatória

Antes de alterar qualquer arquivo, leia nesta ordem:

1. `aicontext/README.md`
2. `aicontext/use-sempre-que-desenvolver.md`
3. `aicontext/use-para-entender-o-proposito-e-arquitetura.md`
4. O guia específico da tarefa:
   - REST, OpenAPI, MCP, CLI e skill: `aicontext/use-quando-desenvolver-api.md`
   - Banco, `Store`, migrações, FTS5 e sqlite-vec: `aicontext/use-quando-desenvolver-persistencia.md`
   - Redator, embeddings, webhooks e workers: `aicontext/use-quando-integrar-provedores.md`
   - Docker, Litestream e reverse proxy: `aicontext/use-quando-configurar-deploy.md`
5. As seções relacionadas de `docs/design.md`.

Quando um arquivo obrigatório ainda não existir, registre a lacuna antes de implementar. Não invente uma regra silenciosamente.

## Propósito e limites

- O Engramind é uma rede de memórias para agentes de IA: guarda fatos duráveis e curtos em **canais**, identifica cada agente por um **token** com escopos e entrega as memórias por **REST, MCP e CLI**.
- Uma memória pertence ao **canal**, não ao agente que a escreveu. O autor é proveniência.
- O serviço é **desacoplado**: não conhece nenhum sistema consumidor ou gestor específico. Toda integração passa pelos contratos públicos (REST, MCP, CLI e webhooks).
- A política fina (quais ferramentas o agente vê, quando ele escreve) pertence ao sistema consumidor. O serviço aplica escopos e acesso efetivo.
- Fora do escopo: indexar código ou documentos (RAG), executar agentes, conversar com usuários e extrair memórias de conversas.
- Uma instância por banco. Esse é o limite consciente do SQLite; não introduza coordenação entre instâncias.

## Arquitetura

- Linguagem Rust; HTTP com `axum` 0.8 + `tokio` + `tower-http`.
- Um único binário: o servidor (`engramind serve`) e a CLI (`engramind <comando>`).
- **Uma única camada de domínio** (`core`). REST, MCP e CLI são adaptadores finos sobre ela, com as mesmas regras, validações, escopos e códigos de erro. Nunca coloque regra de negócio num adaptador.
- Armazenamento atrás da interface `Store`, implementada hoje em SQLite (`rusqlite` bundled, WAL, FTS5 e `sqlite-vec`). Nenhum módulo fora da implementação do `Store` conhece SQL.
- A CLI é cliente HTTP da API. Só `engramind serve`, `engramind migrate` e `engramind doctor` abrem o banco.
- Workers `tokio` (Redator, embeddings, consolidador, webhooks) consomem a fila em tabela `jobs`; não introduza broker.
- OpenAPI com `utoipa` 5 + `utoipa-swagger-ui`; MCP com `rmcp` (Streamable HTTP, stateless) em `POST /mcp`.
- Comportamento funcional (Redator, embeddings, orçamentos, limites) vive nas **configurações** gerenciadas pela API. Variáveis de ambiente cuidam só de infraestrutura.
- Não adicione dependências, tabelas ou serviços além do desenho sem uma necessidade concreta e uma decisão registrada.

## Segurança

- Todo acesso é por Bearer token `eng.<token_id>.<segredo>`; o segredo é comparado em tempo constante com o hash SHA-256 + `TOKEN_PEPPER` e aparece uma única vez, na emissão.
- Tipos `agent` e `admin`; escopos `read`, `write`, `delete` (`write` e `delete` exigem `read`).
- **Acesso efetivo = escopos do token ∩ acesso da assinatura.** A assinatura restringe, nunca amplia. Aplique isso no `core`, nunca só no adaptador.
- Token `agent` nunca alcança ferramentas administrativas nem cria canais.
- Falhe fechado: token ausente, inválido, expirado ou revogado → `401`; escopo insuficiente → `403 SCOPE_REQUIRED`; canal não assinado → `403 CHANNEL_NOT_SUBSCRIBED`.
- `BOOTSTRAP_ADMIN_TOKEN` só é aceito com o banco vazio.
- Segredos de configuração (chaves de API, segredos de webhook) são cifrados com AES-GCM (`SECRETS_KEY`), mantidos com `zeroize` em memória e só gravados; a leitura mostra `{ configured, updated_at }`.
- Nunca registre tokens, segredos, chaves de API, `Authorization` ou o texto das memórias em logs, auditoria, erros ou respostas.
- Memória é instrução que outros agentes vão seguir: preserve proveniência, histórico, auditoria e reversão em toda escrita. O Redator trata o texto como dado.
- CORS fechado; `MCP_ALLOWED_HOSTS` protege `/mcp` contra DNS rebinding.
- Segredos nunca entram no Git, na imagem, em fixtures, exemplos ou argumentos de build.

## APIs

- Cada operação existe nas três vias (REST, MCP, CLI) com o mesmo nome base, parâmetros e erros (§13).
- REST versionado em `/v1`. Fora de `/v1`: `/health/live`, `/health/ready`, `/metrics`, `/openapi.json`, `/docs` e `POST /mcp`.
- Erros usam o envelope único `{ error: { code, message, details? } }`, com `code` estável em inglês e `message` em pt-BR.
- Códigos de erro são parte do contrato: estáveis, acionáveis e documentados no guia de erros.
- Valide path, query, headers e body no `core` antes de tocar no banco ou em provedores.
- Listas são paginadas (`cursor`, `limit`) e aceitam `fields` (`title`/`full`).
- Mutações aceitam `Idempotency-Key`; edições aceitam `expected_version` (`409 MEMORY_VERSION_CONFLICT`).
- A escrita nunca espera o LLM nem o embedding: persiste, enfileira e responde.

## OpenAPI e documentação viva

- Toda rota tem `#[utoipa::path]`, com `operationId` único e estável, tag, resumo, descrição, todos os status e schemas reutilizáveis derivados dos mesmos tipos Rust usados na validação.
- Operações protegidas declaram o security scheme Bearer; rotas públicas declaram a ausência dele.
- `docs/openapi.json` é versionado e precisa ser igual ao gerado (`UPDATE_SNAPSHOTS=1` regenera).
- Guias em `docs/guides/*.md` são compilados no binário (`include_str!`) com público (`all`/`admin`) e tags OpenAPI; o MCP serve a documentação da versão em execução.
- Testes de contrato (§15) garantem: snapshot do OpenAPI, anotação em toda rota, toda ferramenta MCP com operação REST e guia, todo comando da CLI/skill existente em `engramind help --json`, todo código de erro e toda chave de configuração documentados.
- Uma operação não está concluída enquanto REST, OpenAPI, MCP, CLI, guias e skill não estiverem sincronizados.

## Observabilidade

- `tracing` estruturado com `request_id`, `app`, `agent`, operação e duração; campo `event` com nome estável.
- Cada request gera um único evento de conclusão no middleware; handlers de leitura não duplicam logs de sucesso.
- Use a rota parametrizada, nunca a URL concreta ou a query string.
- Healthchecks, `/docs`, OpenAPI, `get_context` com `304` e polls ficam em `DEBUG` quando não houver falha.
- `DEBUG` para diagnóstico, `INFO` para startup e transições concluídas, `WARN` para retry, fallback e degradação recuperável, `ERROR` para falha definitiva.
- Nunca o texto das memórias em nível `INFO` ou acima; nunca segredos em nível algum.
- Formato (`LOG_FORMAT=compact|json`, `LOG_ANSI`) pertence ao subscriber; valores inválidos impedem o startup.
- Nunca use `println!`, `eprintln!` ou `dbg!` no servidor. A saída da CLI fica concentrada num único módulo de saída, com a exceção de lint justificada nele.
- Métricas Prometheus em `/metrics` (§20).

## Qualidade obrigatória

- `cargo fmt --check` e `cargo clippy --all-targets -- -D warnings` passam.
- Lints do crate: `print_stdout`, `print_stderr` e `dbg_macro` negados; `unsafe_code = "deny"`, com um único bloco `unsafe` (registro do `sqlite-vec`) isolado num módulo com `#[allow(unsafe_code)]` e comentário de justificativa.
- Comportamento novo tem teste; correção de bug tem teste de regressão.
- Testes usam SQLite temporário por teste e provedores falsos determinísticos (LLM roteirizado, embeddings por hash, servidor de webhook de teste). Nunca dependem de rede ou de provedor real.
- A matriz token × assinatura × operação cobre escopos e acesso efetivo.
- Execute primeiro o menor conjunto de verificações que valide a mudança (teste do módulo, regressão, lint, contrato). Rode a suíte completa só quando o usuário pedir ou quando a mudança atingir muitas áreas e o impacto não puder ser delimitado; na dúvida, informe o risco residual.

## Fluxo de trabalho

1. Leia o contexto obrigatório e a seção do desenho afetada.
2. Confirme o estado real do repositório (o que existe de fato, não o que o desenho prevê).
3. Implemente a menor mudança correta, no `core` primeiro e depois nos adaptadores.
4. Atualize documentação, guias, skill e OpenAPI que representam o comportamento.
5. Adicione ou ajuste testes, incluindo os de contrato.
6. Atualize a versão conforme a seção Versionamento.
7. Execute formatação, Clippy e os testes aplicáveis.

## Documentação

- `aicontext/`: regras operacionais concisas para desenvolvimento assistido.
- `docs/design.md`: desenho do sistema; fonte de verdade enquanto a implementação não cobrir uma área.
- `docs/guides/`: guias de uso compilados no binário e servidos pelo MCP.
- `docs/decisions/`: decisões arquiteturais e consequências.
- `docs/openapi.json`: snapshot do contrato.
- `skills/engramind/SKILL.md`: skill da CLI.
- `CHANGELOG.md`: mudanças por versão.
- Documente o estado atual, separando fatos implementados, decisões tomadas e propostas futuras. Documente nomes de variáveis, nunca valores reais.

## Versionamento

- SemVer, começando em `0.1.0`. `Cargo.toml` é a única fonte da versão; OpenAPI (`info.version`), `get_status` e `get_overview` usam `CARGO_PKG_VERSION`.
- Toda alteração de código incrementa a versão no mesmo trabalho e registra a entrada no `CHANGELOG.md`.
- `PATCH` para correção ou ajuste interno sem mudança de contrato; `MINOR` para nova capacidade, campo, operação ou comportamento público; `MAJOR` para quebra de contrato. Em `0.x`, quebra de contrato incrementa `MINOR`.
- Atualize `Cargo.lock` junto (`cargo update -p engramind --offline`).
- Crie a tag `vX.Y.Z` somente quando o usuário pedir uma release.
