//! Persistence layer: connection pool, migrations, repositories.
//!
//! Mandantentrennung (Whitepaper §19/§20): every query is scoped by `org_id`.
//! No repository function reads or writes without an org context.
//!
//! The walking skeleton uses runtime-checked queries (`query_as`) so the
//! workspace builds without a live database. WP1.4 can move hot paths to
//! compile-time-checked `query!` once an offline cache is set up.

pub mod residency;

use anyhow::Result;
use chrono::{DateTime, Utc};
use residency::{default_storage_target, ConnectionProvider, SinglePoolProvider};
use serde_json::Value;
use sitegraph_domain::{
    AuditEntry, ChannelAssignment, ContentObject, ContentType, ContentVersion, Membership, Role,
    Status, User, Website,
};
use sitegraph_platform_data_residency::{DataResidencyError, StorageContext};
use sqlx::postgres::PgPoolOptions;
use sqlx::{FromRow, PgPool};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct Db {
    /// Default global pool. Repositories use this until per-context routing
    /// lands (O4.3); kept public for backward compatibility.
    pub pool: PgPool,
    /// Data-residency connection provider (default: single pool, O4.1).
    provider: Arc<dyn ConnectionProvider>,
}

impl Db {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        let provider: Arc<dyn ConnectionProvider> = Arc::new(
            SinglePoolProvider::new(pool.clone(), default_storage_target("default"))
                .map_err(|error| anyhow::anyhow!(error.to_string()))?,
        );
        Ok(Self { pool, provider })
    }

    /// Resolve the pool for a storage context (request path; no Kubernetes).
    /// Today always the single global pool; multi-target routing arrives in O4.2.
    pub fn pool_for(&self, context: &StorageContext) -> Result<PgPool, DataResidencyError> {
        self.provider.pool_for(context)
    }

    /// Apply all migrations in `/migrations` (relative to the workspace root).
    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("../../migrations").run(&self.pool).await?;
        Ok(())
    }
}

// --- Row structs (DB-shaped) mapped to domain types ------------------------

#[derive(FromRow)]
struct ContentObjectRow {
    id: Uuid,
    org_id: Uuid,
    content_type_id: Uuid,
    status: String,
    data: Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ContentObjectRow> for ContentObject {
    fn from(r: ContentObjectRow) -> Self {
        ContentObject {
            id: r.id,
            org_id: r.org_id,
            content_type_id: r.content_type_id,
            status: Status::parse(&r.status),
            data: r.data,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(FromRow)]
struct ContentTypeRow {
    id: Uuid,
    org_id: Uuid,
    key: String,
    name: String,
    schema: Value,
}

impl From<ContentTypeRow> for ContentType {
    fn from(r: ContentTypeRow) -> Self {
        ContentType {
            id: r.id,
            org_id: r.org_id,
            key: r.key,
            name: r.name,
            schema: r.schema,
        }
    }
}

#[derive(FromRow)]
struct ContentVersionRow {
    id: Uuid,
    content_object_id: Uuid,
    data: Value,
    created_at: DateTime<Utc>,
}

impl From<ContentVersionRow> for ContentVersion {
    fn from(r: ContentVersionRow) -> Self {
        ContentVersion {
            id: r.id,
            content_object_id: r.content_object_id,
            data: r.data,
            created_at: r.created_at,
        }
    }
}

#[derive(FromRow)]
struct WebsiteRow {
    id: Uuid,
    org_id: Uuid,
    name: String,
    slug: String,
    domain: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<WebsiteRow> for Website {
    fn from(r: WebsiteRow) -> Self {
        Website {
            id: r.id,
            org_id: r.org_id,
            name: r.name,
            slug: r.slug,
            domain: r.domain,
            created_at: r.created_at,
        }
    }
}

/// Write an immutable snapshot of `data` within an existing transaction (WP1.5).
async fn insert_version(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    content_object_id: Uuid,
    data: &Value,
) -> Result<()> {
    sqlx::query("INSERT INTO content_versions (content_object_id, data) VALUES ($1, $2)")
        .bind(content_object_id)
        .bind(data)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

// --- Content type lookups --------------------------------------------------

impl Db {
    pub async fn content_type_by_key(&self, org_id: Uuid, key: &str) -> Result<Option<ContentType>> {
        let row = sqlx::query_as::<_, ContentTypeRow>(
            "SELECT id, org_id, key, name, schema FROM content_types \
             WHERE org_id = $1 AND key = $2",
        )
        .bind(org_id)
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    pub async fn list_content_types(&self, org_id: Uuid) -> Result<Vec<ContentType>> {
        let rows = sqlx::query_as::<_, ContentTypeRow>(
            "SELECT id, org_id, key, name, schema FROM content_types \
             WHERE org_id = $1 ORDER BY name",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn list_websites(&self, org_id: Uuid) -> Result<Vec<Website>> {
        let rows = sqlx::query_as::<_, WebsiteRow>(
            "SELECT id, org_id, name, slug, domain, created_at FROM websites \
             WHERE org_id = $1 ORDER BY name",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn create_website(
        &self,
        org_id: Uuid,
        name: &str,
        slug: &str,
        domain: Option<&str>,
    ) -> Result<Website> {
        let row = sqlx::query_as::<_, WebsiteRow>(
            "INSERT INTO websites (org_id, name, slug, domain) VALUES ($1, $2, $3, $4) \
             RETURNING id, org_id, name, slug, domain, created_at",
        )
        .bind(org_id)
        .bind(name)
        .bind(slug)
        .bind(domain)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.into())
    }
}

// --- Content CRUD (internal/admin API) -------------------------------------

impl Db {
    pub async fn list_content(&self, org_id: Uuid, type_key: &str) -> Result<Vec<ContentObject>> {
        let rows = sqlx::query_as::<_, ContentObjectRow>(
            "SELECT co.id, co.org_id, co.content_type_id, co.status, co.data, \
                    co.created_at, co.updated_at \
             FROM content_objects co \
             JOIN content_types ct ON ct.id = co.content_type_id \
             WHERE co.org_id = $1 AND ct.key = $2 \
             ORDER BY co.updated_at DESC",
        )
        .bind(org_id)
        .bind(type_key)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn get_content(&self, org_id: Uuid, id: Uuid) -> Result<Option<ContentObject>> {
        let row = sqlx::query_as::<_, ContentObjectRow>(
            "SELECT id, org_id, content_type_id, status, data, created_at, updated_at \
             FROM content_objects WHERE org_id = $1 AND id = $2",
        )
        .bind(org_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    pub async fn create_content(
        &self,
        org_id: Uuid,
        content_type_id: Uuid,
        data: &Value,
    ) -> Result<ContentObject> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query_as::<_, ContentObjectRow>(
            "INSERT INTO content_objects (org_id, content_type_id, status, data) \
             VALUES ($1, $2, 'draft', $3) \
             RETURNING id, org_id, content_type_id, status, data, created_at, updated_at",
        )
        .bind(org_id)
        .bind(content_type_id)
        .bind(data)
        .fetch_one(&mut *tx)
        .await?;
        insert_version(&mut tx, row.id, &row.data).await?;
        tx.commit().await?;
        Ok(row.into())
    }

    pub async fn update_content_data(
        &self,
        org_id: Uuid,
        id: Uuid,
        data: &Value,
    ) -> Result<Option<ContentObject>> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query_as::<_, ContentObjectRow>(
            "UPDATE content_objects SET data = $3, updated_at = now() \
             WHERE org_id = $1 AND id = $2 \
             RETURNING id, org_id, content_type_id, status, data, created_at, updated_at",
        )
        .bind(org_id)
        .bind(id)
        .bind(data)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(r) = &row {
            insert_version(&mut tx, r.id, &r.data).await?;
        }
        tx.commit().await?;
        Ok(row.map(Into::into))
    }

    pub async fn set_status(
        &self,
        org_id: Uuid,
        id: Uuid,
        status: Status,
    ) -> Result<Option<ContentObject>> {
        let row = sqlx::query_as::<_, ContentObjectRow>(
            "UPDATE content_objects SET status = $3, updated_at = now() \
             WHERE org_id = $1 AND id = $2 \
             RETURNING id, org_id, content_type_id, status, data, created_at, updated_at",
        )
        .bind(org_id)
        .bind(id)
        .bind(status.as_str())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    pub async fn delete_content(&self, org_id: Uuid, id: Uuid) -> Result<bool> {
        let res = sqlx::query("DELETE FROM content_objects WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }
}

// --- Channel assignments + delivery ----------------------------------------

impl Db {
    /// Replace the full set of channel assignments for an object (idempotent).
    pub async fn set_channel_assignments(
        &self,
        org_id: Uuid,
        content_object_id: Uuid,
        assignments: &[ChannelAssignment],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // Guard: the object must belong to this org.
        let owns: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM content_objects WHERE org_id = $1 AND id = $2")
                .bind(org_id)
                .bind(content_object_id)
                .fetch_optional(&mut *tx)
                .await?;
        if owns.is_none() {
            tx.rollback().await?;
            anyhow::bail!("content object not found in org");
        }

        sqlx::query("DELETE FROM content_channel_assignments WHERE content_object_id = $1")
            .bind(content_object_id)
            .execute(&mut *tx)
            .await?;

        for a in assignments {
            // Guard: each target website must belong to the same org.
            sqlx::query(
                "INSERT INTO content_channel_assignments (content_object_id, website_id, overrides) \
                 SELECT $1, w.id, $3 FROM websites w WHERE w.id = $2 AND w.org_id = $4",
            )
            .bind(content_object_id)
            .bind(a.website_id)
            .bind(a.overrides.as_ref())
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_channel_assignments(
        &self,
        org_id: Uuid,
        content_object_id: Uuid,
    ) -> Result<Vec<ChannelAssignment>> {
        let rows: Vec<(Uuid, Option<Value>)> = sqlx::query_as(
            "SELECT cca.website_id, cca.overrides \
             FROM content_channel_assignments cca \
             JOIN content_objects co ON co.id = cca.content_object_id \
             WHERE cca.content_object_id = $1 AND co.org_id = $2",
        )
        .bind(content_object_id)
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(website_id, overrides)| ChannelAssignment {
                website_id,
                overrides,
            })
            .collect())
    }

    /// Version history of an object, newest first (org-scoped, WP1.5).
    pub async fn list_versions(
        &self,
        org_id: Uuid,
        content_object_id: Uuid,
    ) -> Result<Vec<ContentVersion>> {
        let rows = sqlx::query_as::<_, ContentVersionRow>(
            "SELECT cv.id, cv.content_object_id, cv.data, cv.created_at \
             FROM content_versions cv \
             JOIN content_objects co ON co.id = cv.content_object_id \
             WHERE cv.content_object_id = $1 AND co.org_id = $2 \
             ORDER BY cv.created_at DESC",
        )
        .bind(content_object_id)
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// Restore an object's `data` from a past version, recording a new version
    /// (WP1.5). Returns `None` if the version doesn't belong to the object/org.
    pub async fn revert(
        &self,
        org_id: Uuid,
        content_object_id: Uuid,
        version_id: Uuid,
    ) -> Result<Option<ContentObject>> {
        let mut tx = self.pool.begin().await?;

        let snapshot: Option<(Value,)> = sqlx::query_as(
            "SELECT cv.data FROM content_versions cv \
             JOIN content_objects co ON co.id = cv.content_object_id \
             WHERE cv.id = $1 AND cv.content_object_id = $2 AND co.org_id = $3",
        )
        .bind(version_id)
        .bind(content_object_id)
        .bind(org_id)
        .fetch_optional(&mut *tx)
        .await?;

        let Some((data,)) = snapshot else {
            tx.rollback().await?;
            return Ok(None);
        };

        let row = sqlx::query_as::<_, ContentObjectRow>(
            "UPDATE content_objects SET data = $3, updated_at = now() \
             WHERE org_id = $1 AND id = $2 \
             RETURNING id, org_id, content_type_id, status, data, created_at, updated_at",
        )
        .bind(org_id)
        .bind(content_object_id)
        .bind(&data)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(r) = &row {
            insert_version(&mut tx, r.id, &r.data).await?;
        }
        tx.commit().await?;
        Ok(row.map(Into::into))
    }

    /// Public delivery (Pull, Whitepaper §13): published objects of `type_key`
    /// assigned to `website_id`, returned as raw `data` plus the per-channel
    /// `overrides` (merging is done by the caller). The website is resolved and
    /// authorized by the caller via the API token.
    pub async fn delivery_list(
        &self,
        website_id: Uuid,
        type_key: &str,
    ) -> Result<Vec<(Value, Option<Value>)>> {
        let rows: Vec<(Value, Option<Value>)> = sqlx::query_as(
            "SELECT co.data, cca.overrides \
             FROM content_objects co \
             JOIN content_channel_assignments cca ON cca.content_object_id = co.id \
             JOIN content_types ct ON ct.id = co.content_type_id \
             WHERE cca.website_id = $1 AND ct.key = $2 AND co.status = 'published' \
             ORDER BY co.updated_at DESC",
        )
        .bind(website_id)
        .bind(type_key)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn website_by_org_slug(&self, org_id: Uuid, slug: &str) -> Result<Option<Website>> {
        let row = sqlx::query_as::<_, WebsiteRow>(
            "SELECT id, org_id, name, slug, domain, created_at FROM websites \
             WHERE org_id = $1 AND slug = $2",
        )
        .bind(org_id)
        .bind(slug)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }
}

// --- Audit log (WP1.4) -----------------------------------------------------

#[derive(FromRow)]
struct AuditRow {
    id: Uuid,
    user_id: Option<Uuid>,
    action: String,
    target_type: String,
    target_id: Option<Uuid>,
    meta: Option<Value>,
    created_at: DateTime<Utc>,
}

impl From<AuditRow> for AuditEntry {
    fn from(r: AuditRow) -> Self {
        AuditEntry {
            id: r.id,
            user_id: r.user_id,
            action: r.action,
            target_type: r.target_type,
            target_id: r.target_id,
            meta: r.meta,
            created_at: r.created_at,
        }
    }
}

impl Db {
    /// Append an audit record (best-effort caller; org-scoped).
    pub async fn insert_audit(
        &self,
        org_id: Uuid,
        user_id: Option<Uuid>,
        action: &str,
        target_type: &str,
        target_id: Option<Uuid>,
        meta: Option<&Value>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO audit_log (org_id, user_id, action, target_type, target_id, meta) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(org_id)
        .bind(user_id)
        .bind(action)
        .bind(target_type)
        .bind(target_id)
        .bind(meta)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_audit(&self, org_id: Uuid, limit: i64) -> Result<Vec<AuditEntry>> {
        let rows = sqlx::query_as::<_, AuditRow>(
            "SELECT id, user_id, action, target_type, target_id, meta, created_at \
             FROM audit_log WHERE org_id = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(org_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}

// --- Auth: users, memberships, API tokens (WP1.3) --------------------------

/// A user plus its password hash, for login verification only.
pub struct UserWithHash {
    pub user: User,
    pub password_hash: String,
}

#[derive(FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    name: String,
    password_hash: String,
}

impl Db {
    pub async fn user_with_hash_by_email(&self, email: &str) -> Result<Option<UserWithHash>> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, name, password_hash FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| UserWithHash {
            user: User {
                id: r.id,
                email: r.email,
                name: r.name,
            },
            password_hash: r.password_hash,
        }))
    }

    pub async fn user_by_id(&self, id: Uuid) -> Result<Option<User>> {
        let row = sqlx::query_as::<_, UserRow>(
            "SELECT id, email, name, password_hash FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| User {
            id: r.id,
            email: r.email,
            name: r.name,
        }))
    }

    pub async fn list_memberships(&self, user_id: Uuid) -> Result<Vec<Membership>> {
        let rows: Vec<(Uuid, String)> =
            sqlx::query_as("SELECT org_id, role FROM memberships WHERE user_id = $1")
                .bind(user_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(org_id, role)| {
                Role::parse(&role).map(|role| Membership { org_id, role })
            })
            .collect())
    }

    /// The user's role in an org, or `None` if not a member (authorization).
    pub async fn membership_role(&self, org_id: Uuid, user_id: Uuid) -> Result<Option<Role>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT role FROM memberships WHERE org_id = $1 AND user_id = $2",
        )
        .bind(org_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|(r,)| Role::parse(&r)))
    }

    /// Idempotent upsert used by the demo bootstrap.
    pub async fn upsert_user(
        &self,
        id: Uuid,
        email: &str,
        name: &str,
        password_hash: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO users (id, email, name, password_hash) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(id)
        .bind(email)
        .bind(name)
        .bind(password_hash)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn upsert_membership(&self, org_id: Uuid, user_id: Uuid, role: Role) -> Result<()> {
        sqlx::query(
            "INSERT INTO memberships (org_id, user_id, role) VALUES ($1, $2, $3) \
             ON CONFLICT (org_id, user_id) DO UPDATE SET role = EXCLUDED.role",
        )
        .bind(org_id)
        .bind(user_id)
        .bind(role.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Create an API token (stores only the hash). Returns nothing; the caller
    /// holds the plaintext.
    pub async fn create_api_token(
        &self,
        org_id: Uuid,
        website_id: Option<Uuid>,
        name: &str,
        token_hash: &str,
    ) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO api_tokens (org_id, website_id, name, token_hash) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(org_id)
        .bind(website_id)
        .bind(name)
        .bind(token_hash)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    /// Idempotent token upsert by hash, used by the demo bootstrap.
    pub async fn upsert_api_token(
        &self,
        org_id: Uuid,
        website_id: Option<Uuid>,
        name: &str,
        token_hash: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO api_tokens (org_id, website_id, name, token_hash) \
             VALUES ($1, $2, $3, $4) ON CONFLICT (token_hash) DO NOTHING",
        )
        .bind(org_id)
        .bind(website_id)
        .bind(name)
        .bind(token_hash)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_api_token(&self, org_id: Uuid, id: Uuid) -> Result<bool> {
        let res = sqlx::query("DELETE FROM api_tokens WHERE org_id = $1 AND id = $2")
            .bind(org_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn list_api_tokens(&self, org_id: Uuid) -> Result<Vec<(Uuid, String, Option<Uuid>)>> {
        let rows: Vec<(Uuid, String, Option<Uuid>)> = sqlx::query_as(
            "SELECT id, name, website_id FROM api_tokens WHERE org_id = $1 ORDER BY created_at",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Look up a token by hash and stamp `last_used_at`. Returns the token's
    /// org and (optional) channel scope.
    pub async fn consume_api_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<(Uuid, Option<Uuid>)>> {
        let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
            "UPDATE api_tokens SET last_used_at = now() WHERE token_hash = $1 \
             RETURNING org_id, website_id",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }
}
