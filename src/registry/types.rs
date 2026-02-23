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
}
