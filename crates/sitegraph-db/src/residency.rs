//! Data-residency seam in the persistence layer (O4.1).
//!
//! Establishes the request-path port that maps a [`StorageContext`] to a
//! `PgPool`. The default [`SinglePoolProvider`] keeps the existing single global
//! pool, but routes through a `SingleTargetResolver` so context validation,
//! Ready-gating and org/region compatibility already apply. No Kubernetes calls
//! happen on this path (O4 acceptance). Multi-target pool caching (O4.2) and
//! per-repository context threading (O4.3) build on this trait.

use sqlx::PgPool;
use sitegraph_platform_data_residency::{
    DataClassification, DataResidencyError, IsolationTopology, SingleTargetResolver,
    StorageContext, StorageResolver, StorageTarget, StorageTargetStatus,
};

/// Resolves the connection pool for a storage context (request path).
pub trait ConnectionProvider: Send + Sync {
    /// Pool for the given context, or an error if no ready target matches.
    fn pool_for(&self, context: &StorageContext) -> Result<PgPool, DataResidencyError>;
    /// The default pool, for repositories not yet routed by context (O4.3).
    fn default_pool(&self) -> &PgPool;
}

/// Default provider: a single global pool fronted by a `SingleTargetResolver`.
pub struct SinglePoolProvider {
    pool: PgPool,
    resolver: SingleTargetResolver,
}

impl SinglePoolProvider {
    pub fn new(pool: PgPool, target: StorageTarget) -> Result<Self, DataResidencyError> {
        Ok(Self {
            pool,
            resolver: SingleTargetResolver::new(target)?,
        })
    }
}

impl ConnectionProvider for SinglePoolProvider {
    fn pool_for(&self, context: &StorageContext) -> Result<PgPool, DataResidencyError> {
        // Resolve to validate the context and enforce Ready/compatibility, then
        // return the one pool — preserving the default binding's behavior.
        self.resolver.resolve(context)?;
        Ok(self.pool.clone())
    }

    fn default_pool(&self) -> &PgPool {
        &self.pool
    }
}

/// The implicit target representing the single global database.
pub fn default_storage_target(region: &str) -> StorageTarget {
    StorageTarget {
        id: "default".into(),
        topology: IsolationTopology::SharedRow,
        region: region.into(),
        status: StorageTargetStatus::Ready,
        endpoint_ref: "default".into(),
        credential_ref: "default".into(),
        organization_id: None,
        schema_version: None,
        revision: 1,
    }
}

/// A storage context for an org with default domain/category/classification and
/// no region pin (the single pool serves every region). Richer contexts arrive
/// with per-repository routing in O4.3.
pub fn default_storage_context(organization_id: &str) -> StorageContext {
    StorageContext {
        organization_id: organization_id.to_string(),
        domain: "content".to_string(),
        data_category: "default".to_string(),
        classification: DataClassification::Internal,
        requested_region: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lazy pool: parses the URL but never connects, so these tests need no DB.
    fn lazy_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://user:pass@localhost/db")
            .expect("valid url")
    }

    // `connect_lazy` spawns a pool reaper, so a Tokio runtime must be present.
    #[tokio::test]
    async fn default_provider_routes_to_the_single_pool() {
        let provider =
            SinglePoolProvider::new(lazy_pool(), default_storage_target("default")).unwrap();
        assert!(provider.pool_for(&default_storage_context("org-a")).is_ok());
    }

    #[tokio::test]
    async fn non_ready_target_is_rejected_on_the_request_path() {
        let mut target = default_storage_target("default");
        target.status = StorageTargetStatus::Provisioning;
        let provider = SinglePoolProvider::new(lazy_pool(), target).unwrap();
        assert!(matches!(
            provider.pool_for(&default_storage_context("org-a")),
            Err(DataResidencyError::TargetNotReady { .. })
        ));
    }

    #[tokio::test]
    async fn tenant_pinned_target_rejects_a_different_org() {
        let mut target = default_storage_target("default");
        target.organization_id = Some("org-b".into());
        let provider = SinglePoolProvider::new(lazy_pool(), target).unwrap();
        assert!(provider
            .pool_for(&default_storage_context("org-a"))
            .is_err());
    }
}
