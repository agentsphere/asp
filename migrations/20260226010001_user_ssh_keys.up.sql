CREATE TABLE user_ssh_keys (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    algorithm       TEXT NOT NULL,
    fingerprint     TEXT NOT NULL UNIQUE,
    public_key_openssh TEXT NOT NULL,
    last_used_at    TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_user_ssh_keys_user ON user_ssh_keys(user_id);
