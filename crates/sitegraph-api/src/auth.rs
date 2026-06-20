//! Authentication primitives (WP1.3): password hashing, JWTs, API tokens.
//!
//! Security notes (Whitepaper §19): passwords are argon2-hashed, API tokens are
//! stored only as SHA-256 hashes, and JWTs are short-lived with a separate
//! long-lived refresh token.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sitegraph_db::Db;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// Fixed demo identifiers (match seed migrations 0001/0002). Replaced by real
// signup in Phase 2 (WP2.3).
pub const DEMO_ORG_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0001);
pub const DEMO_USER_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_000A);
const DEMO_PASSWORD: &str = "demo1234";
const DEMO_API_TOKEN: &str = "sg_demo_token";

const ACCESS_TTL_SECS: u64 = 15 * 60;
const REFRESH_TTL_SECS: u64 = 30 * 24 * 60 * 60;

// --- Passwords -------------------------------------------------------------

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("hash error: {e}"))
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

// --- JWTs ------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject: user id.
    pub sub: Uuid,
    /// "access" | "refresh".
    pub typ: String,
    /// Expiry (unix seconds).
    pub exp: usize,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn issue(secret: &str, user_id: Uuid, typ: &str, ttl: u64) -> anyhow::Result<String> {
    let claims = Claims {
        sub: user_id,
        typ: typ.to_string(),
        exp: (now_secs() + ttl) as usize,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(Into::into)
}

pub fn issue_access(secret: &str, user_id: Uuid) -> anyhow::Result<String> {
    issue(secret, user_id, "access", ACCESS_TTL_SECS)
}

pub fn issue_refresh(secret: &str, user_id: Uuid) -> anyhow::Result<String> {
    issue(secret, user_id, "refresh", REFRESH_TTL_SECS)
}

pub fn verify(secret: &str, token: &str) -> Option<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|d| d.claims)
}

// --- API tokens ------------------------------------------------------------

/// Generate a new opaque API token (the plaintext is shown to the user once).
pub fn generate_api_token() -> String {
    let mut bytes = [0u8; 24];
    OsRng.fill_bytes(&mut bytes);
    format!("sg_{}", hex::encode(bytes))
}

/// Hash an API token for storage/lookup (SHA-256, hex).
pub fn hash_api_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    hex::encode(digest)
}

// --- Demo bootstrap --------------------------------------------------------

/// Idempotently ensure a demo owner + an org-wide delivery token exist, so the
/// skeleton is usable immediately. Removed once real signup lands (WP2.3).
pub async fn bootstrap_demo(db: &Db) -> anyhow::Result<()> {
    let hash = hash_password(DEMO_PASSWORD)?;
    db.upsert_user(DEMO_USER_ID, "owner@demo.test", "Demo Owner", &hash)
        .await?;
    db.upsert_membership(DEMO_ORG_ID, DEMO_USER_ID, sitegraph_domain::Role::Owner)
        .await?;
    db.upsert_api_token(
        DEMO_ORG_ID,
        None, // org-wide so the demo can read every channel
        "demo-delivery",
        &hash_api_token(DEMO_API_TOKEN),
    )
    .await?;
    tracing::info!(
        "demo bootstrap ready (login owner@demo.test / {DEMO_PASSWORD}, delivery token {DEMO_API_TOKEN})"
    );
    Ok(())
}
