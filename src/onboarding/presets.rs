// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

/// Organization type selected during the onboarding wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrgType {
    Solo,
    Startup,
    TechOrg,
    Exploring,
}

impl OrgType {
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solo => "solo",
            Self::Startup => "startup",
            Self::TechOrg => "tech_org",
            Self::Exploring => "exploring",
        }
    }
}

/// Security policy derived from org type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPolicy {
    pub passkey_enforcement: PasskeyPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasskeyPolicy {
    Optional,
    Recommended,
    Mandatory,
}

/// Runtime preset configuration derived from org type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetConfig {
    pub org_type: OrgType,
    pub pipeline_concurrency: usize,
    pub team_workspace: bool,
    pub strict_netpols: bool,
}

impl PresetConfig {
    /// Build the preset config for the given org type.
    pub fn for_org_type(org_type: OrgType) -> Self {
        match org_type {
            OrgType::Solo | OrgType::Exploring => Self {
                org_type,
                pipeline_concurrency: 2,
                team_workspace: false,
                strict_netpols: false,
            },
            OrgType::Startup => Self {
                org_type,
                pipeline_concurrency: 4,
                team_workspace: true,
                strict_netpols: false,
            },
            OrgType::TechOrg => Self {
                org_type,
                pipeline_concurrency: 8,
                team_workspace: true,
                strict_netpols: true,
            },
        }
    }
}

/// Default security policy for each org type.
pub fn default_security_policy(org_type: OrgType) -> SecurityPolicy {
    SecurityPolicy {
        passkey_enforcement: match org_type {
            OrgType::Solo | OrgType::Exploring => PasskeyPolicy::Optional,
            OrgType::Startup => PasskeyPolicy::Recommended,
            OrgType::TechOrg => PasskeyPolicy::Mandatory,
        },
    }
}

/// Apply all preset settings to `platform_settings`, overwriting existing keys.
#[tracing::instrument(skip(pool), err)]
pub async fn apply_preset(pool: &PgPool, org_type: OrgType) -> Result<(), sqlx::Error> {
    let preset = PresetConfig::for_org_type(org_type);
    let security = default_security_policy(org_type);

    upsert_setting(pool, "org_type", &serde_json::json!(org_type)).await?;
    upsert_setting(pool, "preset_config", &serde_json::json!(preset)).await?;
    upsert_setting(pool, "security_policy", &serde_json::json!(security)).await?;

    Ok(())
}

/// Mark the wizard as completed.
pub async fn mark_wizard_completed(pool: &PgPool) -> Result<(), sqlx::Error> {
    upsert_setting(pool, "onboarding_completed", &serde_json::json!(true)).await
}

/// Check if the wizard has been completed.
pub async fn is_wizard_completed(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let val: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT value FROM platform_settings WHERE key = 'onboarding_completed'",
    )
    .fetch_optional(pool)
    .await?;
    Ok(val.and_then(|v| v.as_bool()).unwrap_or(false))
}

/// Read a setting by key.
pub async fn get_setting(
    pool: &PgPool,
    key: &str,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT value FROM platform_settings WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await
}

/// Upsert a setting (public for use by `demo_project`).
pub async fn upsert_setting_pub(
    pool: &PgPool,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    upsert_setting(pool, key, value).await
}

/// Upsert a setting.
async fn upsert_setting(
    pool: &PgPool,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO platform_settings (key, value, updated_at) VALUES ($1, $2, now())
         ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = now()",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// Read the effective pipeline concurrency from `platform_settings` with env fallback.
#[allow(dead_code, clippy::cast_possible_truncation)]
pub async fn effective_pipeline_concurrency(pool: &PgPool, env_default: usize) -> usize {
    let val = get_setting(pool, "preset_config").await.ok().flatten();
    val.and_then(|v| v.get("pipeline_concurrency")?.as_u64())
        .map_or(env_default, |n| n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_solo() {
        let p = PresetConfig::for_org_type(OrgType::Solo);
        assert_eq!(p.pipeline_concurrency, 2);
        assert!(!p.team_workspace);
        assert!(!p.strict_netpols);
    }

    #[test]
    fn preset_startup() {
        let p = PresetConfig::for_org_type(OrgType::Startup);
        assert_eq!(p.pipeline_concurrency, 4);
        assert!(p.team_workspace);
        assert!(!p.strict_netpols);
    }

    #[test]
    fn preset_tech_org() {
        let p = PresetConfig::for_org_type(OrgType::TechOrg);
        assert_eq!(p.pipeline_concurrency, 8);
        assert!(p.team_workspace);
        assert!(p.strict_netpols);
    }

    #[test]
    fn preset_exploring() {
        let p = PresetConfig::for_org_type(OrgType::Exploring);
        assert_eq!(p.pipeline_concurrency, 2);
        assert!(!p.team_workspace);
        assert!(!p.strict_netpols);
    }

    #[test]
    fn security_policy_solo() {
        let s = default_security_policy(OrgType::Solo);
        assert_eq!(s.passkey_enforcement, PasskeyPolicy::Optional);
    }

    #[test]
    fn security_policy_startup() {
        let s = default_security_policy(OrgType::Startup);
        assert_eq!(s.passkey_enforcement, PasskeyPolicy::Recommended);
    }

    #[test]
    fn security_policy_tech_org() {
        let s = default_security_policy(OrgType::TechOrg);
        assert_eq!(s.passkey_enforcement, PasskeyPolicy::Mandatory);
    }

    #[test]
    fn security_policy_exploring() {
        let s = default_security_policy(OrgType::Exploring);
        assert_eq!(s.passkey_enforcement, PasskeyPolicy::Optional);
    }

    #[test]
    fn org_type_as_str() {
        assert_eq!(OrgType::Solo.as_str(), "solo");
        assert_eq!(OrgType::Startup.as_str(), "startup");
        assert_eq!(OrgType::TechOrg.as_str(), "tech_org");
        assert_eq!(OrgType::Exploring.as_str(), "exploring");
    }

    #[test]
    fn org_type_round_trip() {
        for org in [
            OrgType::Solo,
            OrgType::Startup,
            OrgType::TechOrg,
            OrgType::Exploring,
        ] {
            let json = serde_json::to_string(&org).unwrap();
            let back: OrgType = serde_json::from_str(&json).unwrap();
            assert_eq!(org, back);
        }
    }

    #[test]
    fn preset_config_serializes() {
        let p = PresetConfig::for_org_type(OrgType::Startup);
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["pipeline_concurrency"], 4);
        assert_eq!(json["team_workspace"], true);
    }

    #[test]
    fn preset_config_deserializes() {
        let json = r#"{"org_type":"startup","pipeline_concurrency":4,"team_workspace":true,"strict_netpols":false}"#;
        let p: PresetConfig = serde_json::from_str(json).unwrap();
        assert_eq!(p.org_type, OrgType::Startup);
        assert_eq!(p.pipeline_concurrency, 4);
        assert!(p.team_workspace);
        assert!(!p.strict_netpols);
    }

    #[test]
    fn security_policy_serializes() {
        let s = default_security_policy(OrgType::TechOrg);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["passkey_enforcement"], "mandatory");
    }

    #[test]
    fn security_policy_deserializes() {
        let json = r#"{"passkey_enforcement":"optional"}"#;
        let s: SecurityPolicy = serde_json::from_str(json).unwrap();
        assert_eq!(s.passkey_enforcement, PasskeyPolicy::Optional);
    }

    #[test]
    fn passkey_policy_all_variants_serialize() {
        assert_eq!(
            serde_json::to_string(&PasskeyPolicy::Optional).unwrap(),
            "\"optional\""
        );
        assert_eq!(
            serde_json::to_string(&PasskeyPolicy::Recommended).unwrap(),
            "\"recommended\""
        );
        assert_eq!(
            serde_json::to_string(&PasskeyPolicy::Mandatory).unwrap(),
            "\"mandatory\""
        );
    }

    #[test]
    fn org_type_debug() {
        let debug = format!("{:?}", OrgType::Solo);
        assert_eq!(debug, "Solo");
    }

    #[test]
    fn preset_config_debug() {
        let p = PresetConfig::for_org_type(OrgType::Solo);
        let debug = format!("{p:?}");
        assert!(debug.contains("PresetConfig"));
        assert!(debug.contains("Solo"));
    }

    #[test]
    fn security_policy_debug() {
        let s = default_security_policy(OrgType::Startup);
        let debug = format!("{s:?}");
        assert!(debug.contains("SecurityPolicy"));
        assert!(debug.contains("Recommended"));
    }

    #[test]
    fn preset_config_clone() {
        let p = PresetConfig::for_org_type(OrgType::TechOrg);
        let cloned = p.clone();
        assert_eq!(p.pipeline_concurrency, cloned.pipeline_concurrency);
        assert_eq!(p.team_workspace, cloned.team_workspace);
        assert_eq!(p.strict_netpols, cloned.strict_netpols);
    }

    #[test]
    fn passkey_policy_eq() {
        assert_eq!(PasskeyPolicy::Optional, PasskeyPolicy::Optional);
        assert_ne!(PasskeyPolicy::Optional, PasskeyPolicy::Mandatory);
    }

    #[test]
    fn org_type_copy() {
        let org = OrgType::Solo;
        let org2 = org; // Copy
        assert_eq!(org, org2);
    }
}
