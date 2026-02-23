-- Per-user provider API keys (e.g., Anthropic API key).
-- Encrypted at rest with AES-256-GCM using PLATFORM_MASTER_KEY.
CREATE TABLE user_provider_keys (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider      TEXT NOT NULL DEFAULT 'anthropic',
    encrypted_key BYTEA NOT NULL,
    key_suffix    TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, provider)
);
