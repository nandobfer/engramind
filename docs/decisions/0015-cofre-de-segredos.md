# 0015 — Cofre de segredos com AES-256-GCM

**Data**: 2026-10-31 · **Etapa**: 006 (0.6.0) · **Status**: aceita

## Contexto

O Redator precisa de uma chave de API do provedor, que vive nas configurações (desenho §12) e não pode aparecer em resposta, log, auditoria ou backup legível (§18). A `SECRETS_KEY` (32 bytes em base64) já era validada no startup desde a 001.

## Decisão

- **Cifra (D-003).** AES-256-GCM (`aes-gcm` 0.10, nova dependência) com a `SECRETS_KEY` direto como chave. Formato gravado em `secrets.value_enc`: `v1 || nonce(12) || ciphertext+tag`. Nonce aleatório (`OsRng`) a cada gravação. O nome da chave (`redator.api_key`) é o dado associado: um valor copiado para outra chave não decifra.
- **Catálogo (D-004).** Segredos são entradas de `SETTINGS` com `SettingType::Secret`. `set_secret` só aceita essas chaves (`400 SECRET_UNKNOWN`); `update_settings` recusa-as (`400 SETTING_IS_SECRET`). `get_settings` lista cada segredo com `secret: true`, `configured` e `updated_at`, e `value`/`default` sempre `null`.
- **Leitura.** Só o `core` decifra (`Core::secret`), na hora do uso, num `Zeroizing<String>`. Um valor que não decifra (outra `SECRETS_KEY`) vale como não configurado, com `WARN secrets.decrypt_failed` sem o valor.
- **Auditoria e idempotência.** `secret.set`/`secret.delete` gravam só `{ key, configured, updated_at }`. A resposta idempotente não tem o valor; o `request_hash` (SHA-256 do corpo) é unidirecional.
- **CLI.** `engramind secrets set <chave>` lê o valor só da entrada padrão (nunca de argumento, que ficaria no histórico do shell); `--delete` apaga.

## Consequências

- Rotação da `SECRETS_KEY` fica fora do escopo: trocar a chave exige gravar os segredos de novo.
- O `Store` só vê bytes cifrados; um backup do banco (Litestream) não expõe as chaves sem a `SECRETS_KEY`.
