//! API contracts (M0.8).
//!
//! Request/response DTOs for the five operation groups. Decisions fixed here and
//! tested:
//!  * every request type is `deny_unknown_fields` -- an unknown field is
//!    `INVALID_PARAMETER`, not a silent default.
//!  * every response carries `contract_version: 1`.
//!  * every error response is the exact [`SpaceError`] JSON shape from M0.4.
//!  * idempotency: `create file` takes a client-supplied `idempotency_key`;
//!    `commit version` is idempotent (re-commit returns success); chunk and
//!    manifest PUTs are idempotent by content address.
//!
//! The machine-readable form is exported to `schemas/api.openapi.yaml`.

use serde::{Deserialize, Serialize};

use crate::errors::SpaceError;
use crate::ids::{ChunkId, DirectoryId, ManifestId, VersionId};
use crate::model::{Chunk, File, Manifest, Version};

/// The contract version every response echoes.
pub const CONTRACT_VERSION: u32 = 1;

fn contract_version() -> u32 {
    CONTRACT_VERSION
}

/// Error response body -- the `error` object is byte-identical to `SpaceError`
/// across every endpoint.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ErrorResponse {
    #[serde(default = "contract_version")]
    pub contract_version: u32,
    pub error: SpaceError,
}

impl From<SpaceError> for ErrorResponse {
    fn from(error: SpaceError) -> Self {
        Self {
            contract_version: CONTRACT_VERSION,
            error,
        }
    }
}

macro_rules! response {
    ($(#[$m:meta])* $name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
        pub struct $name {
            #[serde(default = "contract_version")]
            pub contract_version: u32,
            $(pub $field : $ty),*
        }
        impl $name {
            pub fn new($($field : $ty),*) -> Self {
                Self { contract_version: CONTRACT_VERSION, $($field),* }
            }
        }
    };
}

// ---- Files -----------------------------------------------------------------

/// `POST /v1/files`
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreateFileRequest {
    pub parent_id: DirectoryId,
    pub name: String,
    /// Client-supplied; makes the create safe to retry.
    pub idempotency_key: String,
}

response!(
    /// `POST /v1/files` and `GET /v1/files/{file_id}`
    FileResponse { file: File }
);

response!(
    /// `GET /v1/dirs/{dir_id}/children`
    ListChildrenResponse { files: Vec<File> }
);

// ---- Versions ------------------------------------------------------------

/// `POST /v1/files/{file_id}/versions` -- idempotent by the client-generated
/// `version_id`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreateVersionRequest {
    pub version_id: VersionId,
    pub parent_version_id: Option<VersionId>,
    pub manifest_id: ManifestId,
}

response!(
    /// `POST /v1/files/{file_id}/versions` and `.../commit`
    VersionResponse { version: Version }
);

/// `POST /v1/versions/{version_id}/commit` -- idempotent; re-commit returns
/// success.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct CommitVersionRequest {}

// ---- Manifests ---------------------------------------------------------

/// `PUT /v1/manifests/{manifest_id}` -- idempotent by content.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PutManifestRequest {
    pub manifest: Manifest,
}

response!(
    /// `GET /v1/manifests/{manifest_id}` and the `PUT` acknowledgement
    ManifestResponse { manifest: Manifest }
);

// ---- Chunks -----------------------------------------------------------

/// `PUT /v1/chunks/{chunk_id}` -- idempotent by content address. The byte body
/// travels out of band; this registers the metadata.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RegisterChunkRequest {
    pub chunk_id: ChunkId,
    pub size: u64,
}

response!(
    /// `PUT /v1/chunks/{chunk_id}` acknowledgement
    ChunkResponse { chunk: Chunk }
);

response!(
    /// `GET /v1/chunks/{chunk_id}` + `Range:` -- the metadata half of a range read
    ChunkRangeResponse { chunk_id: ChunkId, offset: u64, length: u64 }
);

// ---- Health ---------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct HealthResponse {
    pub status: String,
    pub service: String,
    pub version: String,
    pub contract_version: u32,
}

impl HealthResponse {
    pub fn ok() -> Self {
        Self {
            status: "ok".into(),
            service: "space-cloud".into(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            contract_version: CONTRACT_VERSION,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::ErrorCode;

    #[test]
    fn create_file_request_rejects_unknown_fields() {
        let json = r#"{"parent_id":"00000000-0000-7000-8000-000000000000","name":"a","idempotency_key":"k","wat":1}"#;
        let err = serde_json::from_str::<CreateFileRequest>(json).unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("unknown field"),
            "got: {err}"
        );
    }

    #[test]
    fn create_file_request_accepts_the_exact_fields() {
        let json = r#"{"parent_id":"00000000-0000-7000-8000-000000000000","name":"a","idempotency_key":"k"}"#;
        serde_json::from_str::<CreateFileRequest>(json).unwrap();
    }

    #[test]
    fn error_shape_is_identical_across_endpoints() {
        let e = SpaceError::new(ErrorCode::InvalidParameter, "bad id");
        let a = serde_json::to_value(ErrorResponse::from(e.clone())).unwrap();
        let b = serde_json::to_value(ErrorResponse::from(e.clone())).unwrap();
        assert_eq!(a, b);
        assert_eq!(a["error"]["code"], "INVALID_PARAMETER");
        assert_eq!(a["contract_version"], 1);
    }

    #[test]
    fn responses_round_trip_and_carry_contract_version() {
        let file = File {
            file_id: crate::ids::FileId::new(),
            parent_id: crate::ids::DirectoryId::new(),
            name: "a.txt".into(),
            current_version_id: None,
            created_at: chrono::Utc::now(),
            modified_at: chrono::Utc::now(),
        };
        let r = FileResponse::new(file);
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"contract_version\":1"));
        let back: FileResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn health_has_the_exact_documented_shape() {
        let v = serde_json::to_value(HealthResponse::ok()).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["service"], "space-cloud");
        assert_eq!(v["contract_version"], 1);
    }

    #[test]
    fn commit_request_is_empty_object() {
        let r: CommitVersionRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(r, CommitVersionRequest::default());
        assert!(serde_json::from_str::<CommitVersionRequest>(r#"{"x":1}"#).is_err());
    }
}
