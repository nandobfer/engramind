-- 0.11.0: app cujo token de app criou o canal ao assinar (ciclo de vida, não dono;
-- decisão 0033). Canais criados pelo admin ou antes desta versão ficam com `NULL`.

ALTER TABLE channels ADD COLUMN created_by_app_id TEXT REFERENCES apps (id) ON DELETE SET NULL;
