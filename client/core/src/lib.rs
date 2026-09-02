//! Client core (M0.11).
//!
//! Phase 0 scope: a typed HTTP client for the cloud contract, plus the pieces
//! the client binary needs at startup. **No WinFsp, no mount, no drive letter** --
//! those arrive in Phase 1.
//!
//! [`CloudClient`] is the single place the client speaks the M0.8 contract. It
//! carries an `X-Request-Id` on every call and turns every non-2xx response back
//! into a [`SpaceError`] with the original code.

#![forbid(unsafe_code)]

use contracts::api::{
    ChunkResponse, CreateFileRequest, CreateVersionRequest, ErrorResponse, FileResponse,
    HealthResponse, ListChildrenResponse, ManifestResponse, PutManifestRequest, VersionResponse,
};
use contracts::{
    ChunkId, DirectoryId, ErrorCode, File, FileId, Manifest, ManifestId, RequestId, SpaceError,
    Version, VersionId,
};

pub mod startup;

const REQUEST_ID_HEADER: &str = "x-request-id";

/// A typed client for the SPACE cloud contract.
#[derive(Clone)]
pub struct CloudClient {
    base: String,
    http: reqwest::Client,
}

impl CloudClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    async fn send<T: serde::de::DeserializeOwned>(
        &self,
        rb: reqwest::RequestBuilder,
    ) -> Result<T, SpaceError> {
        let rid = RequestId::new().to_string();
        let resp = rb
            .header(REQUEST_ID_HEADER, &rid)
            .send()
            .await
            .map_err(net_error)?;
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(net_error)?;
        if status.is_success() {
            serde_json::from_slice(&bytes).map_err(|e| {
                SpaceError::new(
                    ErrorCode::ProtocolViolation,
                    format!("bad response body: {e}"),
                )
            })
        } else {
            match serde_json::from_slice::<ErrorResponse>(&bytes) {
                Ok(er) => Err(er.error),
                Err(_) => Err(SpaceError::new(
                    ErrorCode::ProtocolViolation,
                    format!("non-2xx ({status}) with unparseable body"),
                )),
            }
        }
    }

    pub async fn health(&self) -> Result<HealthResponse, SpaceError> {
        self.send(self.http.get(self.url("/health"))).await
    }

    pub async fn create_file(
        &self,
        parent_id: DirectoryId,
        name: &str,
        idempotency_key: &str,
    ) -> Result<File, SpaceError> {
        let body = CreateFileRequest {
            parent_id,
            name: name.to_string(),
            idempotency_key: idempotency_key.to_string(),
        };
        let r: FileResponse = self
            .send(self.http.post(self.url("/v1/files")).json(&body))
            .await?;
        Ok(r.file)
    }

    pub async fn stat_file(&self, file_id: FileId) -> Result<File, SpaceError> {
        let r: FileResponse = self
            .send(self.http.get(self.url(&format!("/v1/files/{file_id}"))))
            .await?;
        Ok(r.file)
    }

    pub async fn list_children(&self, dir_id: DirectoryId) -> Result<Vec<File>, SpaceError> {
        let r: ListChildrenResponse = self
            .send(
                self.http
                    .get(self.url(&format!("/v1/dirs/{dir_id}/children"))),
            )
            .await?;
        Ok(r.files)
    }

    pub async fn register_and_put_chunk(
        &self,
        id: &ChunkId,
        bytes: &[u8],
    ) -> Result<ChunkResponse, SpaceError> {
        self.send(
            self.http
                .put(self.url(&format!("/v1/chunks/{id}")))
                .body(bytes.to_vec()),
        )
        .await
    }

    pub async fn get_chunk_range(
        &self,
        id: &ChunkId,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, SpaceError> {
        let end = offset + length - 1;
        let resp = self
            .http
            .get(self.url(&format!("/v1/chunks/{id}")))
            .header(reqwest::header::RANGE, format!("bytes={offset}-{end}"))
            .header(REQUEST_ID_HEADER, RequestId::new().to_string())
            .send()
            .await
            .map_err(net_error)?;
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(net_error)?;
        if !status.is_success() {
            return Err(serde_json::from_slice::<ErrorResponse>(&bytes)
                .map(|e| e.error)
                .unwrap_or_else(|_| {
                    SpaceError::new(ErrorCode::ProtocolViolation, "range GET failed")
                }));
        }
        if bytes.len() as u64 != length {
            return Err(SpaceError::new(
                ErrorCode::IntegrityLengthMismatch,
                format!(
                    "range GET returned {} bytes, expected {length}",
                    bytes.len()
                ),
            ));
        }
        Ok(bytes.to_vec())
    }

    pub async fn put_manifest(&self, manifest: &Manifest) -> Result<Manifest, SpaceError> {
        let r: ManifestResponse = self
            .send(
                self.http
                    .put(self.url(&format!("/v1/manifests/{}", manifest.manifest_id)))
                    .json(&PutManifestRequest {
                        manifest: manifest.clone(),
                    }),
            )
            .await?;
        Ok(r.manifest)
    }

    pub async fn get_manifest(&self, manifest_id: ManifestId) -> Result<Manifest, SpaceError> {
        let r: ManifestResponse = self
            .send(
                self.http
                    .get(self.url(&format!("/v1/manifests/{manifest_id}"))),
            )
            .await?;
        Ok(r.manifest)
    }

    pub async fn create_version(
        &self,
        file_id: FileId,
        version_id: VersionId,
        parent_version_id: Option<VersionId>,
        manifest_id: ManifestId,
    ) -> Result<Version, SpaceError> {
        let r: VersionResponse = self
            .send(
                self.http
                    .post(self.url(&format!("/v1/files/{file_id}/versions")))
                    .json(&CreateVersionRequest {
                        version_id,
                        parent_version_id,
                        manifest_id,
                    }),
            )
            .await?;
        Ok(r.version)
    }

    pub async fn commit_version(&self, version_id: VersionId) -> Result<Version, SpaceError> {
        let r: VersionResponse = self
            .send(
                self.http
                    .post(self.url(&format!("/v1/versions/{version_id}/commit")))
                    .json(&serde_json::json!({})),
            )
            .await?;
        Ok(r.version)
    }

    /// Fetch every range a manifest names, verify each backing chunk's content
    /// address against the bytes returned, and reassemble the logical file.
    pub async fn reassemble(&self, manifest: &Manifest) -> Result<Vec<u8>, SpaceError> {
        contracts::validate::validate_manifest(manifest)?;
        let mut out = vec![0u8; manifest.total_size as usize];
        for entry in &manifest.entries {
            let bytes = self
                .get_chunk_range(&entry.chunk_id, entry.chunk_offset, entry.length)
                .await?;
            // If this range covers a whole chunk, verify the content address.
            if let Some(size) = manifest.chunk_size(&entry.chunk_id) {
                if entry.chunk_offset == 0 && entry.length == size && !entry.chunk_id.verify(&bytes)
                {
                    return Err(SpaceError::new(
                        ErrorCode::IntegrityHashMismatch,
                        "reassembled chunk does not match its content address",
                    ));
                }
            }
            let at = entry.logical_offset as usize;
            out[at..at + bytes.len()].copy_from_slice(&bytes);
        }
        Ok(out)
    }
}

fn net_error(e: reqwest::Error) -> SpaceError {
    let code = if e.is_timeout() {
        ErrorCode::NetworkTimeout
    } else {
        ErrorCode::NetworkUnavailable
    };
    SpaceError::new(code, e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn client_reports_network_unavailable_when_nothing_listens() {
        let c = CloudClient::new("http://127.0.0.1:1"); // nothing here
        let err = c.health().await.unwrap_err();
        assert!(matches!(
            err.code,
            ErrorCode::NetworkUnavailable | ErrorCode::NetworkTimeout
        ));
    }
}
