use serde::{Deserialize, Serialize};

/// OCI Image Manifest (v2 schema 2 / OCI v1).
/// Used to parse manifests during PUT to extract referenced blobs.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OciManifest {
    pub schema_version: Option<i32>,
    pub media_type: Option<String>,
    pub config: Option<Descriptor>,
    pub layers: Option<Vec<Descriptor>>,
    // OCI image index fields
    pub manifests: Option<Vec<Descriptor>>,
}

/// A content descriptor (blob reference within a manifest).
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub media_type: Option<String>,
    pub digest: String,
    pub size: Option<i64>,
}

/// Response for GET /v2/{name}/tags/list
#[derive(Debug, Serialize)]
pub struct TagListResponse {
    pub name: String,
    pub tags: Vec<String>,
}

/// Upload session stored in Valkey.
#[derive(Debug, Serialize, Deserialize)]
pub struct UploadSession {
    pub repository_id: String,
    pub project_id: String,
    pub user_id: String,
    pub offset: i64,
    pub part_count: i32,
}

/// Known OCI/Docker media types for manifests.
#[allow(dead_code)]
pub const MEDIA_TYPE_OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
#[allow(dead_code)]
pub const MEDIA_TYPE_OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
#[allow(dead_code)]
pub const MEDIA_TYPE_DOCKER_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";
#[allow(dead_code)]
pub const MEDIA_TYPE_DOCKER_MANIFEST_LIST: &str =
    "application/vnd.docker.distribution.manifest.list.v2+json";

/// Check if a content type is a recognized manifest media type.
#[allow(dead_code)]
pub fn is_manifest_media_type(ct: &str) -> bool {
    matches!(
        ct,
        MEDIA_TYPE_OCI_MANIFEST
            | MEDIA_TYPE_OCI_INDEX
            | MEDIA_TYPE_DOCKER_MANIFEST
            | MEDIA_TYPE_DOCKER_MANIFEST_LIST
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_oci_manifest() {
        let json = r#"{
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "size": 1234
            },
            "layers": [
                {
                    "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                    "digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "size": 5678
                }
            ]
        }"#;
        let m: OciManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.schema_version, Some(2));
        assert!(m.config.is_some());
        assert_eq!(m.layers.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn parse_oci_index() {
        let json = r#"{
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": [
                {
                    "mediaType": "application/vnd.oci.image.manifest.v1+json",
                    "digest": "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                    "size": 999
                }
            ]
        }"#;
        let m: OciManifest = serde_json::from_str(json).unwrap();
        assert!(m.manifests.is_some());
        assert_eq!(m.manifests.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn is_manifest_media_type_checks() {
        assert!(is_manifest_media_type(MEDIA_TYPE_OCI_MANIFEST));
        assert!(is_manifest_media_type(MEDIA_TYPE_OCI_INDEX));
        assert!(is_manifest_media_type(MEDIA_TYPE_DOCKER_MANIFEST));
        assert!(is_manifest_media_type(MEDIA_TYPE_DOCKER_MANIFEST_LIST));
        assert!(!is_manifest_media_type("application/octet-stream"));
    }

    #[test]
    fn parse_manifest_missing_optional_fields() {
        let json = r#"{"schemaVersion": 2}"#;
        let m: OciManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.schema_version, Some(2));
        assert!(m.config.is_none());
        assert!(m.layers.is_none());
        assert!(m.manifests.is_none());
        assert!(m.media_type.is_none());
    }

    #[test]
    fn parse_manifest_empty_layers() {
        let json = r#"{"schemaVersion": 2, "layers": []}"#;
        let m: OciManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.layers.as_ref().unwrap().len(), 0);
    }

    #[test]
    fn parse_manifest_multiple_layers() {
        let json = r#"{
            "schemaVersion": 2,
            "layers": [
                {"digest": "sha256:aaa", "size": 100},
                {"digest": "sha256:bbb", "size": 200},
                {"digest": "sha256:ccc", "size": 300}
            ]
        }"#;
        let m: OciManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.layers.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn descriptor_minimal_fields() {
        let json = r#"{"digest": "sha256:abc123"}"#;
        let d: Descriptor = serde_json::from_str(json).unwrap();
        assert_eq!(d.digest, "sha256:abc123");
        assert!(d.media_type.is_none());
        assert!(d.size.is_none());
    }

    #[test]
    fn descriptor_all_fields() {
        let json =
            r#"{"mediaType": "application/octet-stream", "digest": "sha256:abc", "size": 42}"#;
        let d: Descriptor = serde_json::from_str(json).unwrap();
        assert_eq!(d.media_type.as_deref(), Some("application/octet-stream"));
        assert_eq!(d.size, Some(42));
    }

    #[test]
    fn descriptor_serialization_roundtrip() {
        let d = Descriptor {
            media_type: Some("application/vnd.oci.image.layer.v1.tar+gzip".into()),
            digest: "sha256:deadbeef".into(),
            size: Some(1024),
        };
        let json = serde_json::to_string(&d).unwrap();
        let d2: Descriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(d2.digest, d.digest);
        assert_eq!(d2.size, d.size);
    }

    #[test]
    fn tag_list_response_serialization() {
        let resp = TagListResponse {
            name: "myapp".into(),
            tags: vec!["v1".into(), "v2".into(), "latest".into()],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["name"], "myapp");
        assert_eq!(json["tags"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn tag_list_response_empty_tags() {
        let resp = TagListResponse {
            name: "empty-repo".into(),
            tags: vec![],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["tags"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn media_type_constants() {
        assert_eq!(
            MEDIA_TYPE_OCI_MANIFEST,
            "application/vnd.oci.image.manifest.v1+json"
        );
        assert_eq!(
            MEDIA_TYPE_OCI_INDEX,
            "application/vnd.oci.image.index.v1+json"
        );
        assert_eq!(
            MEDIA_TYPE_DOCKER_MANIFEST,
            "application/vnd.docker.distribution.manifest.v2+json"
        );
        assert_eq!(
            MEDIA_TYPE_DOCKER_MANIFEST_LIST,
            "application/vnd.docker.distribution.manifest.list.v2+json"
        );
    }

    #[test]
    fn is_manifest_media_type_unknown() {
        assert!(!is_manifest_media_type("text/plain"));
        assert!(!is_manifest_media_type(""));
        assert!(!is_manifest_media_type("application/json"));
    }

    #[test]
    fn upload_session_debug() {
        let session = UploadSession {
            repository_id: "repo-id".into(),
            project_id: "proj-id".into(),
            user_id: "user-id".into(),
            offset: 0,
            part_count: 0,
        };
        let debug = format!("{session:?}");
        assert!(debug.contains("repo-id"));
        assert!(debug.contains("UploadSession"));
    }

    #[test]
    fn upload_session_serde_roundtrip() {
        let session = UploadSession {
            repository_id: "repo-id".into(),
            project_id: "proj-id".into(),
            user_id: "user-id".into(),
            offset: 1024,
            part_count: 3,
        };
        let json = serde_json::to_string(&session).unwrap();
        let parsed: UploadSession = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.offset, 1024);
        assert_eq!(parsed.part_count, 3);
    }

    #[test]
    fn upload_session_zero_state() {
        let session = UploadSession {
            repository_id: "r".into(),
            project_id: "p".into(),
            user_id: "u".into(),
            offset: 0,
            part_count: 0,
        };
        let json = serde_json::to_string(&session).unwrap();
        let parsed: UploadSession = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.offset, 0);
        assert_eq!(parsed.part_count, 0);
    }

    #[test]
    fn upload_session_large_offset() {
        let session = UploadSession {
            repository_id: "r".into(),
            project_id: "p".into(),
            user_id: "u".into(),
            offset: i64::MAX,
            part_count: i32::MAX,
        };
        let json = serde_json::to_string(&session).unwrap();
        let parsed: UploadSession = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.offset, i64::MAX);
        assert_eq!(parsed.part_count, i32::MAX);
    }

    #[test]
    fn upload_session_preserves_all_fields() {
        let session = UploadSession {
            repository_id: "repo-123".into(),
            project_id: "proj-456".into(),
            user_id: "user-789".into(),
            offset: 512,
            part_count: 5,
        };
        let json = serde_json::to_string(&session).unwrap();
        let parsed: UploadSession = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.repository_id, "repo-123");
        assert_eq!(parsed.project_id, "proj-456");
        assert_eq!(parsed.user_id, "user-789");
    }

    #[test]
    fn oci_manifest_roundtrip() {
        let manifest = OciManifest {
            schema_version: Some(2),
            media_type: Some("application/vnd.oci.image.manifest.v1+json".into()),
            config: Some(Descriptor {
                media_type: Some("application/vnd.oci.image.config.v1+json".into()),
                digest: "sha256:abc".into(),
                size: Some(100),
            }),
            layers: Some(vec![Descriptor {
                media_type: Some("application/vnd.oci.image.layer.v1.tar+gzip".into()),
                digest: "sha256:def".into(),
                size: Some(200),
            }]),
            manifests: None,
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: OciManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.schema_version, Some(2));
        assert!(parsed.config.is_some());
        assert_eq!(parsed.layers.as_ref().unwrap().len(), 1);
        assert!(parsed.manifests.is_none());
    }

    #[test]
    fn tag_list_response_debug() {
        let resp = TagListResponse {
            name: "myapp".into(),
            tags: vec!["v1".into()],
        };
        let debug = format!("{resp:?}");
        assert!(debug.contains("TagListResponse"));
        assert!(debug.contains("myapp"));
    }

    #[test]
    fn descriptor_debug() {
        let d = Descriptor {
            media_type: None,
            digest: "sha256:abc".into(),
            size: None,
        };
        let debug = format!("{d:?}");
        assert!(debug.contains("Descriptor"));
    }

    #[test]
    fn oci_manifest_debug() {
        let m = OciManifest {
            schema_version: Some(2),
            media_type: None,
            config: None,
            layers: None,
            manifests: None,
        };
        let debug = format!("{m:?}");
        assert!(debug.contains("OciManifest"));
    }
}
