// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Registry garbage collection: orphaned blob cleanup.

/// Clean up orphaned blobs: no `blob_links`, created more than 24h ago (grace period).
///
/// Deletes from `MinIO` first, then from DB. Skips DB deletion on storage failure
/// so we retry next cycle.
pub async fn collect_garbage(pool: &sqlx::PgPool, minio: &opendal::Operator) -> anyhow::Result<()> {
    let orphans: Vec<(String, String)> = sqlx::query_as(
        r"SELECT digest, minio_path
           FROM registry_blobs b
           WHERE NOT EXISTS (
               SELECT 1 FROM registry_blob_links bl WHERE bl.blob_digest = b.digest
           )
           AND b.created_at < now() - interval '24 hours'",
    )
    .fetch_all(pool)
    .await?;

    if !orphans.is_empty() {
        tracing::info!(
            count = orphans.len(),
            "registry GC: cleaning orphaned blobs"
        );
    }

    for (digest, minio_path) in &orphans {
        // Delete from MinIO first, then from DB
        if let Err(e) = minio.delete(minio_path).await {
            tracing::warn!(error = %e, %digest, "registry GC: failed to delete blob from storage");
            continue; // Skip DB deletion so we retry next cycle
        }

        if let Err(e) = sqlx::query("DELETE FROM registry_blobs WHERE digest = $1")
            .bind(digest)
            .execute(pool)
            .await
        {
            tracing::warn!(error = %e, %digest, "registry GC: failed to delete blob from DB");
        }
    }

    if !orphans.is_empty() {
        tracing::info!(
            deleted = orphans.len(),
            "registry GC: orphaned blobs cleaned"
        );
    }

    Ok(())
}
