//! HTTP API (axum).
//!
//! Surfaces:
//!   * `/auth/*`     — login / refresh (public).
//!   * `/api/*`      — admin API, requires a valid access JWT; org-scoped routes
//!                     additionally enforce the caller's role (WP1.3).
//!   * `/v1/*`       — public Delivery API, authorized by an API token (§13).
//!   * `/openapi.json` — OpenAPI 3.1 document (WP1.4).
//!
//! Writing actions are recorded in the audit log (WP1.4 / §19).

pub mod auth;
pub mod rate_limit;

use axum::extract::{ConnectInfo, Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use rate_limit::RateLimiter;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sitegraph_db::Db;
use sitegraph_domain::{merge_overrides, validate, ChannelAssignment, ContentType, Role, Status};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;

pub use auth::{bootstrap_demo, DEMO_ORG_ID};

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub jwt_secret: String,
    /// Per-token limiter for the public Delivery API.
    pub delivery_limiter: Arc<RateLimiter>,
    /// Per-IP limiter for login attempts.
    pub login_limiter: Arc<RateLimiter>,
}

/// Authenticated principal, injected into request extensions by [`require_auth`].
#[derive(Clone, Copy)]
pub struct AuthUser {
    pub user_id: Uuid,
}

pub fn router(state: AppState) -> Router {
    let state = Arc::new(state);

    let api = Router::new()
        .route("/me", get(me))
        .route("/orgs/:org_id/content-types", get(list_content_types_handler))
        .route("/orgs/:org_id/websites", get(list_websites).post(create_website))
        .route(
            "/orgs/:org_id/content/:type_key",
            get(list_content).post(create_content),
        )
        .route(
            "/orgs/:org_id/content/:type_key/:id",
            get(get_content_handler).put(update_content).delete(delete_content),
        )
        .route(
            "/orgs/:org_id/content/:type_key/:id/channels",
            get(get_channels).put(set_channels),
        )
        .route("/orgs/:org_id/content/:type_key/:id/publish", post(publish))
        .route("/orgs/:org_id/content/:type_key/:id/unpublish", post(unpublish))
        .route("/orgs/:org_id/content/:type_key/:id/versions", get(list_versions))
        .route(
            "/orgs/:org_id/content/:type_key/:id/revert/:version_id",
            post(revert),
        )
        .route("/orgs/:org_id/tokens", get(list_tokens).post(create_token))
        .route("/orgs/:org_id/tokens/:token_id", delete(delete_token))
        .route("/orgs/:org_id/audit", get(list_audit_handler))
        .layer(axum::middleware::from_fn_with_state(state.clone(), require_auth));

    Router::new()
        .route("/healthz", get(healthz))
        .route("/openapi.json", get(openapi_json))
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
        .nest("/api", api)
        .route("/v1/:website_slug/content/:type_key", get(delivery))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// --- OpenAPI (WP1.4) -------------------------------------------------------

#[derive(OpenApi)]
#[openapi(
    info(title = "SiteGraph API", version = "0.1.0", description = "Open content platform for multiple websites."),
    paths(
        healthz, login, refresh, me,
        list_content_types_handler, list_websites, create_website,
        list_content, get_content_handler, create_content, update_content, delete_content,
        get_channels, set_channels, publish, unpublish, list_versions, revert,
        list_tokens, create_token, delete_token, list_audit_handler, delivery,
    ),
    components(schemas(
        LoginBody, RefreshBody, WebsiteBody, ContentBody, ChannelsBody, CreateTokenBody,
        sitegraph_domain::ContentObject, sitegraph_domain::Status,
        sitegraph_domain::ChannelAssignment, sitegraph_domain::ContentVersion,
        sitegraph_domain::User, sitegraph_domain::Role, sitegraph_domain::Membership,
        sitegraph_domain::ContentType, sitegraph_domain::Website,
        sitegraph_domain::AuditEntry, sitegraph_domain::FieldDef,
    )),
    tags(
        (name = "system"), (name = "auth"), (name = "websites"),
        (name = "content"), (name = "tokens"), (name = "audit"), (name = "delivery"),
    )
)]
struct ApiDoc;

async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}

// --- Error handling --------------------------------------------------------

/// Maps internal errors to HTTP responses without leaking internals (§19).
pub enum ApiError {
    NotFound,
    BadRequest(String),
    Unauthorized,
    Forbidden,
    TooManyRequests,
    Internal(anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized".to_string()),
            ApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden".to_string()),
            ApiError::TooManyRequests => {
                (StatusCode::TOO_MANY_REQUESTS, "rate limit exceeded".to_string())
            }
            ApiError::Internal(e) => {
                tracing::error!(error = %e, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e)
    }
}

type ApiResult<T> = Result<T, ApiError>;

// --- Auth middleware + helpers ---------------------------------------------

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

/// A strong ETag derived from response bytes (first 16 bytes of SHA-256).
fn etag_for(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("\"{}\"", hex::encode(&digest[..16]))
}

/// Validate the access JWT and inject [`AuthUser`]; reject otherwise (§19).
async fn require_auth(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = bearer(req.headers()).ok_or(ApiError::Unauthorized)?;
    let claims = auth::verify(&state.jwt_secret, &token).ok_or(ApiError::Unauthorized)?;
    if claims.typ != "access" {
        return Err(ApiError::Unauthorized);
    }
    req.extensions_mut().insert(AuthUser {
        user_id: claims.sub,
    });
    Ok(next.run(req).await)
}

/// Ensure the user is a member of `org_id` with at least `min` privilege.
async fn authorize(state: &AppState, org_id: Uuid, user_id: Uuid, min: Role) -> ApiResult<Role> {
    let role = state
        .db
        .membership_role(org_id, user_id)
        .await?
        .ok_or(ApiError::Forbidden)?;
    if !role.allows(min) {
        return Err(ApiError::Forbidden);
    }
    Ok(role)
}

/// Best-effort audit write: never fails the request (§19 / WP1.4).
async fn audit(
    state: &AppState,
    org_id: Uuid,
    user_id: Uuid,
    action: &str,
    target_type: &str,
    target_id: Option<Uuid>,
    meta: Option<Value>,
) {
    if let Err(e) = state
        .db
        .insert_audit(org_id, Some(user_id), action, target_type, target_id, meta.as_ref())
        .await
    {
        tracing::warn!(error = %e, action, "audit write failed");
    }
}

// --- System ----------------------------------------------------------------

#[utoipa::path(get, path = "/healthz", tag = "system",
    responses((status = 200, description = "Service is up")))]
async fn healthz() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

// --- Auth endpoints --------------------------------------------------------

#[derive(Deserialize, ToSchema)]
struct LoginBody {
    email: String,
    password: String,
}

#[utoipa::path(post, path = "/auth/login", tag = "auth", request_body = LoginBody,
    responses(
        (status = 200, description = "Access + refresh token and the user"),
        (status = 401, description = "Invalid credentials"),
        (status = 429, description = "Too many login attempts")))]
async fn login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(body): Json<LoginBody>,
) -> ApiResult<Json<Value>> {
    // Throttle brute-force per client IP (WP1.6 / §19).
    if !state.login_limiter.check(&addr.ip().to_string()) {
        return Err(ApiError::TooManyRequests);
    }
    let uwh = state
        .db
        .user_with_hash_by_email(&body.email)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    if !auth::verify_password(&uwh.password_hash, &body.password) {
        return Err(ApiError::Unauthorized);
    }
    let access = auth::issue_access(&state.jwt_secret, uwh.user.id)?;
    let refresh = auth::issue_refresh(&state.jwt_secret, uwh.user.id)?;
    Ok(Json(json!({
        "access_token": access,
        "refresh_token": refresh,
        "user": uwh.user,
    })))
}

#[derive(Deserialize, ToSchema)]
struct RefreshBody {
    refresh_token: String,
}

#[utoipa::path(post, path = "/auth/refresh", tag = "auth", request_body = RefreshBody,
    responses(
        (status = 200, description = "A new access token"),
        (status = 401, description = "Invalid refresh token")))]
async fn refresh(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RefreshBody>,
) -> ApiResult<Json<Value>> {
    let claims = auth::verify(&state.jwt_secret, &body.refresh_token).ok_or(ApiError::Unauthorized)?;
    if claims.typ != "refresh" {
        return Err(ApiError::Unauthorized);
    }
    let access = auth::issue_access(&state.jwt_secret, claims.sub)?;
    Ok(Json(json!({ "access_token": access })))
}

#[utoipa::path(get, path = "/api/me", tag = "auth",
    responses((status = 200, description = "The current user and memberships")))]
async fn me(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
) -> ApiResult<Json<Value>> {
    let user = state
        .db
        .user_by_id(auth.user_id)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let memberships = state.db.list_memberships(auth.user_id).await?;
    Ok(Json(json!({ "user": user, "memberships": memberships })))
}

// --- Websites --------------------------------------------------------------

#[utoipa::path(get, path = "/api/orgs/{org_id}/websites", tag = "websites",
    responses((status = 200, description = "Channels of the org")))]
async fn list_websites(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path(org_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Viewer).await?;
    let sites = state.db.list_websites(org_id).await?;
    Ok(Json(json!(sites)))
}

#[derive(Deserialize, ToSchema)]
struct WebsiteBody {
    name: String,
    slug: String,
    domain: Option<String>,
}

#[utoipa::path(post, path = "/api/orgs/{org_id}/websites", tag = "websites",
    request_body = WebsiteBody,
    responses((status = 200, description = "Created channel"), (status = 403, description = "Owner only")))]
async fn create_website(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path(org_id): Path<Uuid>,
    Json(body): Json<WebsiteBody>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Owner).await?;
    let site = state
        .db
        .create_website(org_id, &body.name, &body.slug, body.domain.as_deref())
        .await?;
    audit(&state, org_id, auth.user_id, "website.create", "website", Some(site.id),
        Some(json!({ "slug": body.slug }))).await;
    Ok(Json(json!(site)))
}

// --- Content types ---------------------------------------------------------

#[utoipa::path(get, path = "/api/orgs/{org_id}/content-types", tag = "content",
    responses((status = 200, description = "Content type definitions (with schema)")))]
async fn list_content_types_handler(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path(org_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Viewer).await?;
    let types = state.db.list_content_types(org_id).await?;
    Ok(Json(json!(types)))
}

// --- Content ---------------------------------------------------------------

/// Resolve a content type (org-scoped) and validate `data` against it (WP1.2).
async fn validated_type(
    state: &AppState,
    org_id: Uuid,
    type_key: &str,
    data: &Value,
) -> ApiResult<ContentType> {
    let ct = state
        .db
        .content_type_by_key(org_id, type_key)
        .await?
        .ok_or_else(|| ApiError::BadRequest(format!("unknown content type '{type_key}'")))?;
    let errors = validate(&ct.schema, data);
    if !errors.is_empty() {
        return Err(ApiError::BadRequest(errors.join("; ")));
    }
    Ok(ct)
}

#[utoipa::path(get, path = "/api/orgs/{org_id}/content/{type_key}", tag = "content",
    responses((status = 200, description = "Content objects of a type")))]
async fn list_content(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path((org_id, type_key)): Path<(Uuid, String)>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Viewer).await?;
    let items = state.db.list_content(org_id, &type_key).await?;
    Ok(Json(json!(items)))
}

#[utoipa::path(get, path = "/api/orgs/{org_id}/content/{type_key}/{id}", tag = "content",
    responses((status = 200, description = "A single content object"), (status = 404, description = "Not found")))]
async fn get_content_handler(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path((org_id, _type_key, id)): Path<(Uuid, String, Uuid)>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Viewer).await?;
    let obj = state.db.get_content(org_id, id).await?.ok_or(ApiError::NotFound)?;
    Ok(Json(json!(obj)))
}

#[derive(Deserialize, ToSchema)]
struct ContentBody {
    data: Value,
}

#[utoipa::path(post, path = "/api/orgs/{org_id}/content/{type_key}", tag = "content",
    request_body = ContentBody,
    responses(
        (status = 200, description = "Created content object"),
        (status = 400, description = "Schema validation failed")))]
async fn create_content(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path((org_id, type_key)): Path<(Uuid, String)>,
    Json(body): Json<ContentBody>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Editor).await?;
    let ct = validated_type(&state, org_id, &type_key, &body.data).await?;
    let obj = state.db.create_content(org_id, ct.id, &body.data).await?;
    audit(&state, org_id, auth.user_id, "content.create", "content", Some(obj.id),
        Some(json!({ "type": type_key }))).await;
    Ok(Json(json!(obj)))
}

#[utoipa::path(put, path = "/api/orgs/{org_id}/content/{type_key}/{id}", tag = "content",
    request_body = ContentBody,
    responses(
        (status = 200, description = "Updated content object"),
        (status = 400, description = "Schema validation failed"),
        (status = 404, description = "Not found")))]
async fn update_content(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path((org_id, type_key, id)): Path<(Uuid, String, Uuid)>,
    Json(body): Json<ContentBody>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Editor).await?;
    validated_type(&state, org_id, &type_key, &body.data).await?;
    let obj = state
        .db
        .update_content_data(org_id, id, &body.data)
        .await?
        .ok_or(ApiError::NotFound)?;
    audit(&state, org_id, auth.user_id, "content.update", "content", Some(obj.id), None).await;
    Ok(Json(json!(obj)))
}

#[utoipa::path(delete, path = "/api/orgs/{org_id}/content/{type_key}/{id}", tag = "content",
    responses((status = 204, description = "Deleted"), (status = 404, description = "Not found")))]
async fn delete_content(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path((org_id, _type_key, id)): Path<(Uuid, String, Uuid)>,
) -> ApiResult<StatusCode> {
    authorize(&state, org_id, auth.user_id, Role::Editor).await?;
    if state.db.delete_content(org_id, id).await? {
        audit(&state, org_id, auth.user_id, "content.delete", "content", Some(id), None).await;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

#[utoipa::path(get, path = "/api/orgs/{org_id}/content/{type_key}/{id}/channels", tag = "content",
    responses((status = 200, description = "Current channel assignments")))]
async fn get_channels(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path((org_id, _type_key, id)): Path<(Uuid, String, Uuid)>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Viewer).await?;
    let assignments = state.db.get_channel_assignments(org_id, id).await?;
    Ok(Json(json!({ "assignments": assignments })))
}

#[derive(Deserialize, ToSchema)]
struct ChannelsBody {
    assignments: Vec<ChannelAssignment>,
}

#[utoipa::path(put, path = "/api/orgs/{org_id}/content/{type_key}/{id}/channels", tag = "content",
    request_body = ChannelsBody,
    responses((status = 200, description = "Current assignments")))]
async fn set_channels(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path((org_id, _type_key, id)): Path<(Uuid, String, Uuid)>,
    Json(body): Json<ChannelsBody>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Editor).await?;
    state
        .db
        .set_channel_assignments(org_id, id, &body.assignments)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    audit(&state, org_id, auth.user_id, "content.channels", "content", Some(id),
        Some(json!({ "count": body.assignments.len() }))).await;
    let current = state.db.get_channel_assignments(org_id, id).await?;
    Ok(Json(json!({ "assignments": current })))
}

#[utoipa::path(post, path = "/api/orgs/{org_id}/content/{type_key}/{id}/publish", tag = "content",
    responses((status = 200, description = "Published object"), (status = 404, description = "Not found")))]
async fn publish(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path((org_id, _type_key, id)): Path<(Uuid, String, Uuid)>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Editor).await?;
    let obj = state
        .db
        .set_status(org_id, id, Status::Published)
        .await?
        .ok_or(ApiError::NotFound)?;
    audit(&state, org_id, auth.user_id, "content.publish", "content", Some(obj.id), None).await;
    Ok(Json(json!(obj)))
}

#[utoipa::path(post, path = "/api/orgs/{org_id}/content/{type_key}/{id}/unpublish", tag = "content",
    responses((status = 200, description = "Unpublished object"), (status = 404, description = "Not found")))]
async fn unpublish(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path((org_id, _type_key, id)): Path<(Uuid, String, Uuid)>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Editor).await?;
    let obj = state
        .db
        .set_status(org_id, id, Status::Draft)
        .await?
        .ok_or(ApiError::NotFound)?;
    audit(&state, org_id, auth.user_id, "content.unpublish", "content", Some(obj.id), None).await;
    Ok(Json(json!(obj)))
}

// --- Versioning (WP1.5) ----------------------------------------------------

#[utoipa::path(get, path = "/api/orgs/{org_id}/content/{type_key}/{id}/versions", tag = "content",
    responses((status = 200, description = "Version history")))]
async fn list_versions(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path((org_id, _type_key, id)): Path<(Uuid, String, Uuid)>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Viewer).await?;
    let versions = state.db.list_versions(org_id, id).await?;
    Ok(Json(json!({ "versions": versions })))
}

#[utoipa::path(post, path = "/api/orgs/{org_id}/content/{type_key}/{id}/revert/{version_id}",
    tag = "content",
    responses((status = 200, description = "Reverted object"), (status = 404, description = "Not found")))]
async fn revert(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path((org_id, _type_key, id, version_id)): Path<(Uuid, String, Uuid, Uuid)>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Editor).await?;
    let obj = state
        .db
        .revert(org_id, id, version_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    audit(&state, org_id, auth.user_id, "content.revert", "content", Some(obj.id),
        Some(json!({ "version_id": version_id }))).await;
    Ok(Json(json!(obj)))
}

// --- API tokens ------------------------------------------------------------

#[utoipa::path(get, path = "/api/orgs/{org_id}/tokens", tag = "tokens",
    responses((status = 200, description = "Tokens (without secrets)"), (status = 403, description = "Owner only")))]
async fn list_tokens(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path(org_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Owner).await?;
    let tokens = state.db.list_api_tokens(org_id).await?;
    let out: Vec<Value> = tokens
        .into_iter()
        .map(|(id, name, website_id)| json!({ "id": id, "name": name, "website_id": website_id }))
        .collect();
    Ok(Json(json!({ "tokens": out })))
}

#[derive(Deserialize, ToSchema)]
struct CreateTokenBody {
    name: String,
    website_id: Option<Uuid>,
}

#[utoipa::path(post, path = "/api/orgs/{org_id}/tokens", tag = "tokens",
    request_body = CreateTokenBody,
    responses((status = 200, description = "Created token; plaintext shown once"), (status = 403, description = "Owner only")))]
async fn create_token(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path(org_id): Path<Uuid>,
    Json(body): Json<CreateTokenBody>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Owner).await?;
    let token = auth::generate_api_token();
    let id = state
        .db
        .create_api_token(org_id, body.website_id, &body.name, &auth::hash_api_token(&token))
        .await?;
    audit(&state, org_id, auth.user_id, "token.create", "token", Some(id),
        Some(json!({ "website_id": body.website_id }))).await;
    Ok(Json(json!({
        "id": id,
        "token": token,
        "note": "store this token now; it is not retrievable later",
    })))
}

#[utoipa::path(delete, path = "/api/orgs/{org_id}/tokens/{token_id}", tag = "tokens",
    responses((status = 204, description = "Revoked"), (status = 404, description = "Not found"), (status = 403, description = "Owner only")))]
async fn delete_token(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path((org_id, token_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    authorize(&state, org_id, auth.user_id, Role::Owner).await?;
    if state.db.delete_api_token(org_id, token_id).await? {
        audit(&state, org_id, auth.user_id, "token.delete", "token", Some(token_id), None).await;
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

// --- Audit (WP1.4) ---------------------------------------------------------

#[derive(Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
}

#[utoipa::path(get, path = "/api/orgs/{org_id}/audit", tag = "audit",
    responses((status = 200, description = "Recent audit entries"), (status = 403, description = "Owner only")))]
async fn list_audit_handler(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthUser>,
    Path(org_id): Path<Uuid>,
    Query(q): Query<AuditQuery>,
) -> ApiResult<Json<Value>> {
    authorize(&state, org_id, auth.user_id, Role::Owner).await?;
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let entries = state.db.list_audit(org_id, limit).await?;
    Ok(Json(json!({ "entries": entries })))
}

// --- Delivery (public Pull, token-authorized) ------------------------------

#[derive(Deserialize)]
struct DeliveryQuery {
    token: Option<String>,
}

#[utoipa::path(get, path = "/v1/{website_slug}/content/{type_key}", tag = "delivery",
    responses(
        (status = 200, description = "Published content for the channel"),
        (status = 304, description = "Not modified (ETag matched)"),
        (status = 401, description = "Missing/invalid token"),
        (status = 403, description = "Token not scoped to this channel"),
        (status = 429, description = "Rate limit exceeded")))]
async fn delivery(
    State(state): State<Arc<AppState>>,
    Path((website_slug, type_key)): Path<(String, String)>,
    headers: HeaderMap,
    Query(q): Query<DeliveryQuery>,
) -> ApiResult<Response> {
    let token = bearer(&headers).or(q.token).ok_or(ApiError::Unauthorized)?;
    let token_hash = auth::hash_api_token(&token);

    // Throttle per token before touching the DB (WP1.6 / §19).
    if !state.delivery_limiter.check(&token_hash) {
        return Err(ApiError::TooManyRequests);
    }

    let (org_id, scope_website) = state
        .db
        .consume_api_token(&token_hash)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    // Resolve the channel within the token's org (cross-org slugs can't leak).
    let site = state
        .db
        .website_by_org_slug(org_id, &website_slug)
        .await?
        .ok_or(ApiError::NotFound)?;

    // Channel-scoped tokens may only read their own channel (WP1.6).
    if let Some(wid) = scope_website {
        if wid != site.id {
            return Err(ApiError::Forbidden);
        }
    }

    let rows = state.db.delivery_list(site.id, &type_key).await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|(data, overrides)| merge_overrides(&data, overrides.as_ref()))
        .collect();

    // Edge/client caching (WP1.6): ETag over the payload + a short max-age.
    let body = json!({ "items": items });
    let payload = serde_json::to_vec(&body).map_err(|e| ApiError::Internal(e.into()))?;
    let etag = etag_for(&payload);

    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        == Some(etag.as_str())
    {
        return Ok((
            StatusCode::NOT_MODIFIED,
            [(header::ETAG, etag), (header::CACHE_CONTROL, cache_control())],
        )
            .into_response());
    }

    Ok((
        StatusCode::OK,
        [
            (header::ETAG, etag),
            (header::CACHE_CONTROL, cache_control()),
            (header::CONTENT_TYPE, "application/json".to_string()),
        ],
        payload,
    )
        .into_response())
}

fn cache_control() -> String {
    "public, max-age=60".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_document_builds() {
        let doc = ApiDoc::openapi();
        let json = serde_json::to_string(&doc).expect("serialize openapi");
        // Spot-check that representative paths made it in.
        assert!(json.contains("/api/orgs/{org_id}/content/{type_key}"));
        assert!(json.contains("/v1/{website_slug}/content/{type_key}"));
        assert!(json.contains("/api/orgs/{org_id}/audit"));
    }
}
