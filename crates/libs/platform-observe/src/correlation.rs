// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use uuid::Uuid;

use crate::proto::{KeyValue, get_string_attr};

/// Correlation metadata extracted from OTLP resource and span attributes.
#[derive(Debug, Clone, Default)]
pub struct CorrelationEnvelope {
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub session_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub service: String,
}

/// Extract correlation fields from resource + span/log attributes.
///
/// Looks for well-known keys: `service.name`, `platform.session_id`,
/// `platform.project_id`, `platform.user_id`.
pub fn extract_correlation(
    resource_attrs: &[KeyValue],
    record_attrs: &[KeyValue],
) -> CorrelationEnvelope {
    let service =
        get_string_attr(resource_attrs, "service.name").unwrap_or_else(|| "unknown".into());

    let session_id = get_string_attr(record_attrs, "platform.session_id")
        .or_else(|| get_string_attr(resource_attrs, "platform.session_id"))
        .and_then(|s| Uuid::parse_str(&s).ok());

    let project_id = get_string_attr(record_attrs, "platform.project_id")
        .or_else(|| get_string_attr(resource_attrs, "platform.project_id"))
        .and_then(|s| Uuid::parse_str(&s).ok());

    let user_id = get_string_attr(record_attrs, "platform.user_id")
        .or_else(|| get_string_attr(resource_attrs, "platform.user_id"))
        .and_then(|s| Uuid::parse_str(&s).ok());

    CorrelationEnvelope {
        trace_id: None,
        span_id: None,
        session_id,
        project_id,
        user_id,
        service,
    }
}

/// Resolve `session_id` to `project_id` and `user_id` from `agent_sessions`.
#[tracing::instrument(skip(pool), fields(?session_id = envelope.session_id), err)]
pub async fn resolve_session(
    pool: &sqlx::PgPool,
    envelope: &mut CorrelationEnvelope,
) -> Result<(), sqlx::Error> {
    let Some(sid) = envelope.session_id else {
        return Ok(());
    };

    if envelope.project_id.is_some() && envelope.user_id.is_some() {
        return Ok(());
    }

    if let Some(row) = sqlx::query!(
        "SELECT project_id, user_id FROM agent_sessions WHERE id = $1",
        sid,
    )
    .fetch_optional(pool)
    .await?
    {
        if envelope.project_id.is_none() {
            envelope.project_id = row.project_id;
        }
        if envelope.user_id.is_none() {
            envelope.user_id = Some(row.user_id);
        }
    } else {
        // Session not found — clear to avoid FK violation on log_entries.session_id
        envelope.session_id = None;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{AnyValue, any_value};

    fn str_kv(key: &str, val: &str) -> KeyValue {
        KeyValue {
            key: key.into(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue(val.into())),
            }),
        }
    }

    #[test]
    fn extract_service_name() {
        let resource = vec![str_kv("service.name", "my-svc")];
        let env = extract_correlation(&resource, &[]);
        assert_eq!(env.service, "my-svc");
    }

    #[test]
    fn extract_default_service() {
        let env = extract_correlation(&[], &[]);
        assert_eq!(env.service, "unknown");
    }

    #[test]
    fn extract_project_id_from_record() {
        let pid = Uuid::new_v4();
        let attrs = vec![str_kv("platform.project_id", &pid.to_string())];
        let env = extract_correlation(&[], &attrs);
        assert_eq!(env.project_id, Some(pid));
    }

    #[test]
    fn extract_project_id_from_resource_fallback() {
        let pid = Uuid::new_v4();
        let resource = vec![str_kv("platform.project_id", &pid.to_string())];
        let env = extract_correlation(&resource, &[]);
        assert_eq!(env.project_id, Some(pid));
    }

    #[test]
    fn invalid_uuid_ignored() {
        let attrs = vec![str_kv("platform.session_id", "not-a-uuid")];
        let env = extract_correlation(&[], &attrs);
        assert_eq!(env.session_id, None);
    }

    // -- session_id extraction --

    #[test]
    fn extract_session_id_from_record() {
        let sid = Uuid::new_v4();
        let attrs = vec![str_kv("platform.session_id", &sid.to_string())];
        let env = extract_correlation(&[], &attrs);
        assert_eq!(env.session_id, Some(sid));
    }

    #[test]
    fn extract_session_id_from_resource_fallback() {
        let sid = Uuid::new_v4();
        let resource = vec![str_kv("platform.session_id", &sid.to_string())];
        let env = extract_correlation(&resource, &[]);
        assert_eq!(env.session_id, Some(sid));
    }

    // -- user_id extraction --

    #[test]
    fn extract_user_id_from_record() {
        let uid = Uuid::new_v4();
        let attrs = vec![str_kv("platform.user_id", &uid.to_string())];
        let env = extract_correlation(&[], &attrs);
        assert_eq!(env.user_id, Some(uid));
    }

    #[test]
    fn extract_user_id_from_resource_fallback() {
        let uid = Uuid::new_v4();
        let resource = vec![str_kv("platform.user_id", &uid.to_string())];
        let env = extract_correlation(&resource, &[]);
        assert_eq!(env.user_id, Some(uid));
    }

    // -- record attrs take precedence over resource attrs --

    #[test]
    fn record_attrs_take_precedence_for_project_id() {
        let record_pid = Uuid::new_v4();
        let resource_pid = Uuid::new_v4();
        let record = vec![str_kv("platform.project_id", &record_pid.to_string())];
        let resource = vec![str_kv("platform.project_id", &resource_pid.to_string())];
        let env = extract_correlation(&resource, &record);
        assert_eq!(env.project_id, Some(record_pid));
    }

    #[test]
    fn record_attrs_take_precedence_for_session_id() {
        let record_sid = Uuid::new_v4();
        let resource_sid = Uuid::new_v4();
        let record = vec![str_kv("platform.session_id", &record_sid.to_string())];
        let resource = vec![str_kv("platform.session_id", &resource_sid.to_string())];
        let env = extract_correlation(&resource, &record);
        assert_eq!(env.session_id, Some(record_sid));
    }

    #[test]
    fn record_attrs_take_precedence_for_user_id() {
        let record_uid = Uuid::new_v4();
        let resource_uid = Uuid::new_v4();
        let record = vec![str_kv("platform.user_id", &record_uid.to_string())];
        let resource = vec![str_kv("platform.user_id", &resource_uid.to_string())];
        let env = extract_correlation(&resource, &record);
        assert_eq!(env.user_id, Some(record_uid));
    }

    // -- all fields extracted simultaneously --

    #[test]
    fn extract_all_fields_at_once() {
        let pid = Uuid::new_v4();
        let sid = Uuid::new_v4();
        let uid = Uuid::new_v4();
        let resource = vec![str_kv("service.name", "my-svc")];
        let record = vec![
            str_kv("platform.project_id", &pid.to_string()),
            str_kv("platform.session_id", &sid.to_string()),
            str_kv("platform.user_id", &uid.to_string()),
        ];
        let env = extract_correlation(&resource, &record);
        assert_eq!(env.service, "my-svc");
        assert_eq!(env.project_id, Some(pid));
        assert_eq!(env.session_id, Some(sid));
        assert_eq!(env.user_id, Some(uid));
    }

    #[test]
    fn invalid_user_id_uuid_ignored() {
        let attrs = vec![str_kv("platform.user_id", "garbage")];
        let env = extract_correlation(&[], &attrs);
        assert_eq!(env.user_id, None);
    }

    #[test]
    fn invalid_project_id_uuid_ignored() {
        let attrs = vec![str_kv("platform.project_id", "garbage")];
        let env = extract_correlation(&[], &attrs);
        assert_eq!(env.project_id, None);
    }
}
