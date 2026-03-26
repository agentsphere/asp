use sqlx::PgPool;
use uuid::Uuid;

/// Handle for fire-and-forget audit logging.
///
/// Each call to [`send_audit`] spawns an independent tokio task that writes to
/// the `audit_log` table. This keeps handler latency unaffected by DB pool
/// pressure while ensuring entries are visible to subsequent queries within
/// the same tokio runtime (the spawned task is polled during the next `.await`).
#[derive(Clone)]
pub struct AuditLog {
    pool: PgPool,
}

impl AuditLog {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

pub struct AuditEntry {
    pub actor_id: Uuid,
    pub actor_name: String,
    pub action: String,
    pub resource: String,
    pub resource_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub detail: Option<serde_json::Value>,
    pub ip_addr: Option<String>,
}

pub fn send_audit(log: &AuditLog, entry: AuditEntry) {
    let pool = log.pool.clone();
    tokio::spawn(async move {
        write_audit_inner(&pool, &entry).await;
    });
}

async fn write_audit_inner(pool: &PgPool, entry: &AuditEntry) {
    let ip: Option<ipnetwork::IpNetwork> = entry.ip_addr.as_deref().and_then(|s| s.parse().ok());

    if let Err(e) = sqlx::query!(
        r#"
        INSERT INTO audit_log (actor_id, actor_name, action, resource, resource_id, project_id, detail, ip_addr)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
        entry.actor_id,
        entry.actor_name,
        entry.action,
        entry.resource,
        entry.resource_id,
        entry.project_id,
        entry.detail,
        ip as Option<ipnetwork::IpNetwork>,
    )
    .execute(pool)
    .await
    {
        tracing::warn!(
            error = %e,
            action = %entry.action,
            resource = %entry.resource,
            "failed to write audit log entry"
        );
    }
}
