-- Reverse registry migration

DROP TRIGGER IF EXISTS trg_registry_tags_updated ON registry_tags;
DROP TRIGGER IF EXISTS trg_registry_repos_updated ON registry_repositories;

DROP TABLE IF EXISTS registry_tags;
DROP TABLE IF EXISTS registry_manifests;
DROP TABLE IF EXISTS registry_blob_links;
DROP TABLE IF EXISTS registry_blobs;
DROP TABLE IF EXISTS registry_repositories;

-- Remove registry permissions from role_permissions
DELETE FROM role_permissions
WHERE permission_id IN (SELECT id FROM permissions WHERE name IN ('registry:pull', 'registry:push'));

-- Remove registry permissions
DELETE FROM permissions WHERE name IN ('registry:pull', 'registry:push');
