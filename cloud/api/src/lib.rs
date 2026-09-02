//! Cloud API surface (M0.8/M0.12).
//!
//! An `axum` router over the five operation groups plus `/health`, backed by the
//! in-memory [`MetadataStore`] and a [`FakeObjectStore`]. Every response carries
//! `contract_version: 1`; every error is the [`ErrorResponse`] JSON shape from
//! M0.4; every request and response carries an `X-Request-Id` (minted if the
//! inbound one is absent or malformed) that is attached to the tracing span.
//!
//! PostgreSQL is Phase 8. This layer only knows the trait.

#![forbid(unsafe_code)]

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};

use contracts::api::{
    ChunkResponse, CreateFileRequest, CreateVersionRequest, ErrorResponse, FileResponse,
    HealthResponse, ListChildrenResponse, ManifestResponse, PutManifestRequest, VersionResponse,
};
use contracts::{ChunkId, DirectoryId, ErrorCode, FileId, ManifestId, SpaceError, VersionId};
use objectstore::{FakeObjectStore, ObjectStore};
use space_cloud_metadata::{InMemoryMetadataStore, MetadataStore};

pub const REQUEST_ID_HEADER: &str = "x-request-id";

pub struct AppState {
    pub meta: InMemoryMetadataStore,
    pub blobs: FakeObjectStore,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            meta: InMemoryMetadataStore::new(),
            blobs: FakeObjectStore::new(),
        }
    }
}

pub type SharedState = Arc<AppState>;

/// Build the full router.
pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/files", post(create_file))
        .route("/v1/files/:file_id", get(stat_file))
        .route("/v1/dirs/:dir_id/children", get(list_children))
        .route("/v1/files/:file_id/versions", post(create_version))
        .route("/v1/versions/:version_id/commit", post(commit_version))
        .route(
            "/v1/manifests/:manifest_id",
            put(put_manifest).get(get_manifest),
        )
        .route("/v1/chunks/:chunk_id", put(put_chunk).get(get_chunk))
        .route("/v1/_slow/:ms", get(slow))
        .fallback(not_found)
        .layer(middleware::from_fn(request_id_layer))
        .with_state(state)
}

// ---- request-id middleware ------------------------------------------------

async fn request_id_layer(mut req: Request, next: Next) -> Response {
    let inbound = req
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|s| contracts::RequestId::parse(s).is_ok())
        .map(|s| s.to_string());

    let request_id = inbound.unwrap_or_else(|| contracts::RequestId::new().to_string());
    let span = tracing::info_span!("request", request_id = %request_id);
    let _enter = span.enter();

    if let Ok(hv) = HeaderValue::from_str(&request_id) {
        req.headers_mut().insert(REQUEST_ID_HEADER, hv.clone());
        let mut res = next.run(req).await;
        res.headers_mut().insert(REQUEST_ID_HEADER, hv);
        res
    } else {
        next.run(req).await
    }
}

// ---- error mapping -------------------------------------------------------

fn status_for(code: ErrorCode) -> StatusCode {
    use ErrorCode::*;
    match code {
        FileNotFound | VersionNotFound | ManifestNotFound | ChunkNotFound => StatusCode::NOT_FOUND,
        FileExists | DirectoryNotEmpty | SharingViolation | IntegrityChunkIdConflict => {
            StatusCode::CONFLICT
        }
        InvalidParameter
        | InvalidHandle
        | ProtocolViolation
        | ConfigInvalid
        | ConfigMissing
        | ConfigUnsupportedVersion => StatusCode::BAD_REQUEST,
        IntegrityHashMismatch | IntegrityLengthMismatch | IntegrityManifestInvalid => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        AuthFailed => StatusCode::UNAUTHORIZED,
        PermissionDenied => StatusCode::FORBIDDEN,
        NetworkTimeout | NetworkUnavailable => StatusCode::GATEWAY_TIMEOUT,
        ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
        DiskFull | StorageError | Cancelled | InternalError => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

struct ApiError(SpaceError);

impl From<SpaceError> for ApiError {
    fn from(e: SpaceError) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (status_for(self.0.code), Json(ErrorResponse::from(self.0))).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn parse_file_id(s: &str) -> Result<FileId, ApiError> {
    FileId::parse(s).map_err(ApiError::from)
}
fn parse_dir_id(s: &str) -> Result<DirectoryId, ApiError> {
    DirectoryId::parse(s).map_err(ApiError::from)
}
fn parse_version_id(s: &str) -> Result<VersionId, ApiError> {
    VersionId::parse(s).map_err(ApiError::from)
}
fn parse_manifest_id(s: &str) -> Result<ManifestId, ApiError> {
    ManifestId::parse(s).map_err(ApiError::from)
}
fn parse_chunk_id(s: &str) -> Result<ChunkId, ApiError> {
    ChunkId::parse(s).map_err(ApiError::from)
}

// ---- handlers ----------------------------------------------------------

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse::ok())
}

async fn not_found() -> ApiError {
    ApiError(SpaceError::new(ErrorCode::FileNotFound, "no such route"))
}

async fn create_file(
    State(st): State<SharedState>,
    Json(req): Json<CreateFileRequest>,
) -> ApiResult<Json<FileResponse>> {
    let file = st
        .meta
        .create_file(req.parent_id, &req.name, &req.idempotency_key)
        .await?;
    Ok(Json(FileResponse::new(file)))
}

async fn stat_file(
    State(st): State<SharedState>,
    Path(file_id): Path<String>,
) -> ApiResult<Json<FileResponse>> {
    let file = st.meta.get_file(parse_file_id(&file_id)?).await?;
    Ok(Json(FileResponse::new(file)))
}

async fn list_children(
    State(st): State<SharedState>,
    Path(dir_id): Path<String>,
) -> ApiResult<Json<ListChildrenResponse>> {
    let files = st.meta.list_children(parse_dir_id(&dir_id)?).await?;
    Ok(Json(ListChildrenResponse::new(files)))
}

async fn create_version(
    State(st): State<SharedState>,
    Path(file_id): Path<String>,
    Json(req): Json<CreateVersionRequest>,
) -> ApiResult<Json<VersionResponse>> {
    let version = st
        .meta
        .create_version(
            parse_file_id(&file_id)?,
            req.version_id,
            req.parent_version_id,
            req.manifest_id,
        )
        .await?;
    Ok(Json(VersionResponse::new(version)))
}

async fn commit_version(
    State(st): State<SharedState>,
    Path(version_id): Path<String>,
) -> ApiResult<Json<VersionResponse>> {
    let version = st
        .meta
        .commit_version(parse_version_id(&version_id)?)
        .await?;
    Ok(Json(VersionResponse::new(version)))
}

async fn put_manifest(
    State(st): State<SharedState>,
    Path(manifest_id): Path<String>,
    Json(req): Json<PutManifestRequest>,
) -> ApiResult<Json<ManifestResponse>> {
    let id = parse_manifest_id(&manifest_id)?;
    if req.manifest.manifest_id != id {
        return Err(ApiError(SpaceError::invalid_param(
            "manifest_id in path and body disagree",
        )));
    }
    let manifest = st.meta.put_manifest(req.manifest).await?;
    Ok(Json(ManifestResponse::new(manifest)))
}

async fn get_manifest(
    State(st): State<SharedState>,
    Path(manifest_id): Path<String>,
) -> ApiResult<Json<ManifestResponse>> {
    let manifest = st
        .meta
        .get_manifest(parse_manifest_id(&manifest_id)?)
        .await?;
    Ok(Json(ManifestResponse::new(manifest)))
}

async fn put_chunk(
    State(st): State<SharedState>,
    Path(chunk_id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<ChunkResponse>> {
    let id = parse_chunk_id(&chunk_id)?;
    st.blobs.put(&id, &body).await?;
    let chunk = st.meta.register_chunk(id, body.len() as u64).await?;
    Ok(Json(ChunkResponse::new(chunk)))
}

async fn get_chunk(
    State(st): State<SharedState>,
    Path(chunk_id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let id = parse_chunk_id(&chunk_id)?;
    let size = st.blobs.head(&id).await?;
    match headers.get(axum::http::header::RANGE) {
        None => {
            let bytes = st.blobs.get(&id).await?;
            Ok(bytes.into_response())
        }
        Some(range) => {
            let (start, end_inclusive) = parse_range(range.to_str().unwrap_or(""), size)
                .ok_or_else(|| ApiError(SpaceError::invalid_param("malformed Range header")))?;
            let length = (end_inclusive - start + 1) as u32;
            let bytes = st.blobs.get_range(&id, start, length).await?;
            let mut res = bytes.into_response();
            *res.status_mut() = StatusCode::PARTIAL_CONTENT;
            res.headers_mut().insert(
                axum::http::header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes {start}-{end_inclusive}/{size}")).unwrap(),
            );
            Ok(res)
        }
    }
}

/// `bytes=<start>-<end>` (end optional). Returns an inclusive `(start, end)`.
/// Only the *syntax* and `start <= end` are checked here; whether the range fits
/// the stored object is the object store's decision (a short object is an
/// integrity fault, not a malformed request).
fn parse_range(raw: &str, size: u64) -> Option<(u64, u64)> {
    let spec = raw.strip_prefix("bytes=")?;
    let (s, e) = spec.split_once('-')?;
    let start: u64 = s.parse().ok()?;
    let end: u64 = if e.is_empty() {
        size.max(1) - 1
    } else {
        e.parse().ok()?
    };
    if start > end {
        return None;
    }
    Some((start, end))
}

async fn slow(Path(ms): Path<u64>) -> &'static str {
    tokio::time::sleep(std::time::Duration::from_millis(ms.min(60_000))).await;
    "done"
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_has_exact_shape_and_echoes_request_id() {
        let app = router(Arc::new(AppState::default()));
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header(REQUEST_ID_HEADER, "r_00000000-0000-7000-8000-000000000001")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(REQUEST_ID_HEADER).unwrap(),
            "r_00000000-0000-7000-8000-000000000001"
        );
        let body = axum::body::to_bytes(res.into_body(), 64 * 1024)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["contract_version"], 1);
    }

    #[tokio::test]
    async fn malformed_request_id_is_replaced_not_rejected() {
        let app = router(Arc::new(AppState::default()));
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header(REQUEST_ID_HEADER, "not-a-valid-id")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let got = res
            .headers()
            .get(REQUEST_ID_HEADER)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(got.starts_with("r_") && got != "not-a-valid-id");
    }

    #[tokio::test]
    async fn unknown_route_is_structured_error() {
        let app = router(Arc::new(AppState::default()));
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/nope")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(res.into_body(), 64 * 1024)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["error"]["code"], "FILE_NOT_FOUND");
        assert_eq!(v["contract_version"], 1);
    }

    #[test]
    fn range_parsing() {
        assert_eq!(parse_range("bytes=0-9", 100), Some((0, 9)));
        assert_eq!(parse_range("bytes=90-", 100), Some((90, 99)));
        // out-of-range is syntactically valid; the store rejects it
        assert_eq!(parse_range("bytes=90-200", 100), Some((90, 200)));
        assert_eq!(parse_range("bytes=9-2", 100), None);
        assert_eq!(parse_range("0-9", 100), None);
    }
}
