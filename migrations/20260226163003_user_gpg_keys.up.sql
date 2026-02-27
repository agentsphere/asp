CREATE TABLE user_gpg_keys (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    key_id           TEXT NOT NULL,
    fingerprint      TEXT NOT NULL UNIQUE,
    public_key_armor TEXT NOT NULL,
    public_key_bytes BYTEA NOT NULL,
    emails           TEXT[] NOT NULL DEFAULT '{}',
    expires_at       TIMESTAMPTZ,
    can_sign         BOOLEAN NOT NULL DEFAULT true,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_user_gpg_keys_user ON user_gpg_keys(user_id);
CREATE INDEX idx_user_gpg_keys_key_id ON user_gpg_keys(key_id);
CREATE INDEX idx_user_gpg_keys_emails ON user_gpg_keys USING GIN (emails);
