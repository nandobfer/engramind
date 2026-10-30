# 0007 — Tokens `eng.tk_….<segredo>` com HMAC-SHA256

**Data**: 2026-10-30 · **Etapa**: 002 (0.2.0) · **Status**: aceita

## Contexto

O desenho (§4) pedia `eng.<token_id>.<segredo>` no esquema do sap-middleman e falava em "SHA-256 com o pepper". O sap-middleman usa HMAC-SHA256 com o pepper como chave e um UUID como id. O desenho (§6) também pede ids públicos curtos com prefixo de tipo (`tk_…`).

## Decisão

- Formato: `eng.tk_<8 hex minúsculos>.<segredo>`. Um único identificador: o id público do token (`tk_3f9e1a7c`) é o `token_id` do texto.
- Segredo: 32 bytes de `OsRng` em base64url sem padding (43 caracteres). O parser é estrito: esquema `eng`, id `tk_` + 8 hex e segredo com 32 ou mais caracteres `[A-Za-z0-9_-]`.
- Guarda: `secret_hash = HMAC-SHA256(chave = TOKEN_PEPPER, mensagem = segredo)`, conferido com `Mac::verify_slice` (tempo constante). O HMAC é verificado mesmo para tokens inativos, para não revelar o estado pelo tempo de resposta.
- `TokenService` (`src/auth/token.rs`) é portado do sap-middleman, com o esquema `eng` e o id `tk_`. O verificador do pepper continua em `core/pepper.rs` (0001), com o mesmo HMAC.
- `BOOTSTRAP_ADMIN_TOKEN` usa o mesmo formato; valor fora dele impede o startup.

## Consequências

- Trocar o `TOKEN_PEPPER` invalida todos os tokens; por isso o startup recusa um pepper divergente.
- O id de 32 bits aleatórios é gerado com retentativa em colisão dentro da transação.
- O desenho §4 e §16 e o `AGENTS.md` passam a dizer HMAC-SHA256.
