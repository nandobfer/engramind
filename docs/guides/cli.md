# CLI

O binário `engramind` (também `eng`) é ao mesmo tempo o servidor e o cliente. `serve`, `migrate` e `doctor` abrem o banco; os demais comandos são clientes HTTP da API, um por operação, com os mesmos parâmetros e códigos de erro do REST.

## Conexão

Cada valor é lido nesta ordem: flag, variável de ambiente, perfil.

| Flag | Variável | Perfil |
|---|---|---|
| `--url` | `ENGRAMIND_URL` | `url` |
| `--token` | `ENGRAMIND_TOKEN` | `token` |
| `--channels` | `ENGRAMIND_CHANNELS` | `channels` |

`--profile` escolhe o perfil; sem ele, vale `default_profile`.

`--channels` (ou `ENGRAMIND_CHANNELS`) vira o cabeçalho `Engramind-Channels` de toda requisição: a lista de slugs e prefixos que restringe os canais da conexão (guia `canais-e-assinaturas`).

## Perfis

Os perfis ficam em `~/.config/engramind/config.toml`, gravado com permissão `0600`:

```toml
default_profile = "curador"

[profiles.curador]
url = "https://engramind.exemplo"
token = "eng.tk_3f9e1a7c.<segredo>"
```

```bash
engramind profiles add curador --url https://engramind.exemplo --token "$TOKEN" --default
engramind profiles list
engramind profiles use curador
engramind profiles remove curador
```

`profiles list` mostra o token mascarado (`eng.<token_id>.***`).

## Saída

- Sem flag, a saída é legível: listas em linhas e objetos em `chave: valor`.
- `--json` funciona em todo comando e imprime o JSON da API. Em erro, imprime o envelope `{ "error": { … } }` no stdout.
- `engramind help --json` descreve todos os comandos, a operação que cada um chama e os códigos de saída.

## Comandos

```bash
engramind whoami
engramind apps list
engramind apps create --slug app-a --name "App A"
engramind agents create --app app-a --slug curador --name Curador
engramind tokens issue --agent app-a/curador --scopes read,write
engramind tokens revoke tk_3f9e1a7c
engramind channels create --slug eng/global --name "Engenharia global"
engramind channels list --include-archived
engramind subscriptions add --agent app-a/curador --target "eng/*" --mode manage --access read,write
engramind subscriptions update sub_7c2e9a10 --token-access
engramind memories create --channel eng/global --category CONSTRAINTS --text "…"
engramind memories search testes locais
engramind memories list --fields title --channels eng/global
engramind channels stats eng/global
engramind channels settings set eng/global memory.text_max 2000
engramind settings get limits.rate_per_minute
engramind settings set limits.rate_per_minute 1200
engramind status
engramind audit list --entity token
engramind docs guide tokens-e-escopos --section Escopos
engramind docs search idempotencia
```

Mutações aceitam `--idempotency-key`. Em `settings set` e `channels settings set`, o valor é lido como JSON (`600`, `true`); `null` volta ao padrão. Em `memories create` e `memories update`, `--text -` lê o texto da entrada padrão. O `--channels` global vira o cabeçalho `Engramind-Channels`: nenhum subcomando tem argumento próprio de canais.

## Códigos de saída

| Saída | Quando |
|---|---|
| 0 | Sucesso |
| 1 | Qualquer outro erro |
| 2 | Uso inválido: argumentos, `VALIDATION_ERROR`, `CATEGORY_NOT_FOUND`, `SETTING_*` ou erro local (`CLI_CONFIG_INVALID`, `CLI_INPUT_INVALID`) |
| 3 | `AUTH_INVALID` |
| 4 | `SCOPE_REQUIRED` ou `CHANNEL_NOT_SUBSCRIBED` |
| 5 | Recurso inexistente (`*_NOT_FOUND`) |
| 6 | Conflito (`409`) |
| 7 | `RATE_LIMITED` |
| 8 | Servidor indisponível ou rede (`DATABASE_UNAVAILABLE`, `MIGRATIONS_PENDING`, `REQUEST_TIMEOUT`, `CLI_CONNECTION_FAILED`) |

O mapa completo, código a código, está no guia `erros`. Mudar um código de saída é quebra de contrato.

## Erros locais

Erros que nascem na própria CLI, antes ou fora da API:

| Código | Saída | Quando |
|---|---|---|
| `CLI_CONFIG_INVALID` | 2 | URL ou token ausentes, perfil inexistente ou arquivo de perfis inválido. |
| `CLI_INPUT_INVALID` | 2 | Falha ao ler o texto da entrada padrão (`--text -`). |
| `CLI_CONNECTION_FAILED` | 8 | Servidor inacessível: rede, DNS, TLS ou tempo limite. |
| `CLI_UNEXPECTED_RESPONSE` | 1 | Resposta fora do contrato (por exemplo, HTML de um proxy). |
