-- OCI image registry: repositories, blobs, manifests, tags

-- Repositories: one per project, lazily created on first push
CREATE TABLE registry_repositories (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_registry_repos_project ON registry_repositories(project_id);

CREATE TRIGGER trg_registry_repos_updated
    BEFORE UPDATE ON registry_repositories FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Blobs: content-addressable, shared across repositories
CREATE TABLE registry_blobs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    digest      TEXT NOT NULL UNIQUE,   -- "sha256:abcdef..."
    size_bytes  BIGINT NOT NULL,
    minio_path  TEXT NOT NULL,          -- "registry/blobs/sha256/{hex}"
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Many-to-many: which repositories reference which blobs
CREATE TABLE registry_blob_links (
    repository_id UUID NOT NULL REFERENCES registry_repositories(id) ON DELETE CASCADE,
    blob_digest   TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (repository_id, blob_digest)
);

-- Manifests: one row per (repository, digest)
CREATE TABLE registry_manifests (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    repository_id   UUID NOT NULL REFERENCES registry_repositories(id) ON DELETE CASCADE,
    digest          TEXT NOT NULL,           -- "sha256:..."
    media_type      TEXT NOT NULL,           -- e.g. "application/vnd.oci.image.manifest.v1+json"
    content         BYTEA NOT NULL,          -- raw manifest JSON
    size_bytes      BIGINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(repository_id, digest)
);

-- Tags: mutable pointer from name to manifest digest
CREATE TABLE registry_tags (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    repository_id   UUID NOT NULL REFERENCES registry_repositories(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    manifest_digest TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(repository_id, name)
);

CREATE TRIGGER trg_registry_tags_updated
    BEFORE UPDATE ON registry_tags FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Seed registry permissions
INSERT INTO permissions (id, name, resource, action, description) VALUES
    (gen_random_uuid(), 'registry:pull', 'registry', 'pull', 'Pull images from project registry'),
    (gen_random_uuid(), 'registry:push', 'registry', 'push', 'Push images to project registry')
ON CONFLICT (name) DO NOTHING;

-- Admin gets all permissions via bootstrap wildcard logic

-- Developer: pull + push
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.name = 'developer' AND p.name IN ('registry:pull', 'registry:push')
ON CONFLICT DO NOTHING;

-- Ops: pull only
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.name = 'ops' AND p.name = 'registry:pull'
ON CONFLICT DO NOTHING;

-- Viewer: pull only
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.name = 'viewer' AND p.name = 'registry:pull'
ON CONFLICT DO NOTHING;

-- Admin: pull + push
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id FROM roles r, permissions p
WHERE r.name = 'admin' AND p.name IN ('registry:pull', 'registry:push')
ON CONFLICT DO NOTHING;
