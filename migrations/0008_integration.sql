-- Etapa 008 (integração): webhook por app e índices da retenção.
-- O segredo do webhook é cifrado com SECRETS_KEY (AES-256-GCM) e nunca é lido de volta pela API.

ALTER TABLE apps ADD COLUMN webhook_url TEXT;
ALTER TABLE apps ADD COLUMN webhook_secret_enc BLOB;
ALTER TABLE apps ADD COLUMN webhook_secret_updated_at TEXT;

-- Retenção por período (FR-014); usage_log, llm_calls e idempotency já têm índice por instante.
CREATE INDEX audit_log_at ON audit_log (at);
CREATE INDEX jobs_finished ON jobs (state, finished_at);
