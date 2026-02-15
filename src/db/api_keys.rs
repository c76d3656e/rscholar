//! API Key management with production-grade security.
//!
//! Security features:
//! - HMAC-SHA256 with server pepper (prevents offline attacks if DB leaks)
//! - Constant-time comparison (prevents timing side-channels)
//! - secrecy crate (prevents accidental key leakage in logs/debug)

use crate::error::{GscholarError, Result};
use hmac::{Hmac, Mac};
use rusqlite::{params, Connection, OptionalExtension};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use tracing::debug;

type HmacSha256 = Hmac<Sha256>;

/// Server-side pepper for HMAC (should be set via env in production)
/// This adds a layer of protection if the database is compromised.
fn get_pepper() -> String {
    std::env::var("RUSTGSCHOLAR_KEY_PEPPER")
        .unwrap_or_else(|_| "rscholar-default-pepper-change-in-production".to_string())
}

/// API Key entity (stored in DB)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    #[serde(skip_serializing)] // Never expose hash
    pub key_hash: String,
    pub name: String,
    pub is_admin: bool,
    pub rate_limit_rps: u32,
    pub request_count: i64,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
}

/// Response when creating a key (includes plaintext key)
#[derive(Clone, Serialize)]
pub struct ApiKeyCreated {
    pub id: String,
    #[serde(serialize_with = "serialize_secret")]
    pub key: SecretString, // Wrapped in secrecy to prevent accidental logging
    pub name: String,
    pub is_admin: bool,
    pub rate_limit_rps: u32,
    pub created_at: i64,
}

// Custom serializer to expose the secret only for JSON response
fn serialize_secret<S>(secret: &SecretString, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(secret.expose_secret())
}

impl std::fmt::Debug for ApiKeyCreated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiKeyCreated")
            .field("id", &self.id)
            .field("key", &"[REDACTED]") // Never log the key
            .field("name", &self.name)
            .field("is_admin", &self.is_admin)
            .finish()
    }
}

/// Generate a new API key
///
/// Returns (SecretString containing plaintext_key, hmac_hash)
pub fn generate_key() -> (SecretString, String) {
    let key = format!("rgs_{}", uuid::Uuid::new_v4().to_string().replace("-", ""));
    let hash = hash_key(&key);
    (SecretString::from(key), hash)
}

/// Hash a key using HMAC-SHA256 with server pepper
///
/// This is more secure than plain SHA256 because:
/// 1. Requires knowledge of pepper to generate valid hashes
/// 2. Protects against rainbow table attacks
pub fn hash_key(key: &str) -> String {
    let pepper = get_pepper();
    let mut mac = HmacSha256::new_from_slice(pepper.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(key.as_bytes());
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

/// Verify a key against a stored hash using constant-time comparison
///
/// This prevents timing side-channel attacks where an attacker could
/// learn information about the hash by measuring response times.
pub fn verify_key_hash(plaintext_key: &str, stored_hash: &str) -> bool {
    let computed_hash = hash_key(plaintext_key);
    let computed_bytes = computed_hash.as_bytes();
    let stored_bytes = stored_hash.as_bytes();
    
    // Constant-time comparison - takes same time regardless of where mismatch occurs
    computed_bytes.ct_eq(stored_bytes).into()
}

/// Create a new API key
pub fn create(conn: &Connection, name: &str, is_admin: bool, rate_limit_rps: u32) -> Result<ApiKeyCreated> {
    let id = format!("key_{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or(""));
    let (key, key_hash) = generate_key();
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "INSERT INTO api_keys (id, key_hash, name, is_admin, rate_limit_rps, request_count, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
        params![id, key_hash, name, is_admin as i32, rate_limit_rps, now],
    ).map_err(|e| GscholarError::Database(format!("Create key failed: {}", e)))?;

    debug!(key_id = %id, name = %name, "API key created");

    Ok(ApiKeyCreated {
        id,
        key,
        name: name.to_string(),
        is_admin,
        rate_limit_rps,
        created_at: now,
    })
}

/// Validate an API key and return key info if valid
///
/// Uses constant-time comparison to prevent timing attacks.
pub fn validate(conn: &Connection, plaintext_key: &str) -> Result<Option<ApiKey>> {
    // First compute the hash
    let key_hash = hash_key(plaintext_key);

    // Query by hash (indexed lookup)
    let key = conn.query_row(
        "SELECT id, key_hash, name, is_admin, rate_limit_rps, request_count, last_used_at, created_at
         FROM api_keys WHERE key_hash = ?1",
        params![key_hash],
        |row| {
            Ok(ApiKey {
                id: row.get(0)?,
                key_hash: row.get(1)?,
                name: row.get(2)?,
                is_admin: row.get::<_, i32>(3)? != 0,
                rate_limit_rps: row.get::<_, i32>(4)? as u32,
                request_count: row.get(5)?,
                last_used_at: row.get(6)?,
                created_at: row.get(7)?,
            })
        },
    ).optional()
    .map_err(|e| GscholarError::Database(format!("Validate key failed: {}", e)))?;

    // Verify with constant-time comparison (defense in depth)
    if let Some(ref found_key) = key {
        if !verify_key_hash(plaintext_key, &found_key.key_hash) {
            // This shouldn't happen if DB is consistent, but safety first
            return Ok(None);
        }
    }

    Ok(key)
}

/// Update key usage (increment count, update last_used_at)
pub fn record_usage(conn: &Connection, key_hash: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "UPDATE api_keys SET request_count = request_count + 1, last_used_at = ?1 WHERE key_hash = ?2",
        params![now, key_hash],
    ).map_err(|e| GscholarError::Database(format!("Record usage failed: {}", e)))?;

    Ok(())
}

/// Get key by ID
pub fn get_by_id(conn: &Connection, id: &str) -> Result<Option<ApiKey>> {
    let key = conn.query_row(
        "SELECT id, key_hash, name, is_admin, rate_limit_rps, request_count, last_used_at, created_at
         FROM api_keys WHERE id = ?1",
        params![id],
        |row| {
            Ok(ApiKey {
                id: row.get(0)?,
                key_hash: row.get(1)?,
                name: row.get(2)?,
                is_admin: row.get::<_, i32>(3)? != 0,
                rate_limit_rps: row.get::<_, i32>(4)? as u32,
                request_count: row.get(5)?,
                last_used_at: row.get(6)?,
                created_at: row.get(7)?,
            })
        },
    ).optional()
    .map_err(|e| GscholarError::Database(format!("Get key failed: {}", e)))?;

    Ok(key)
}

/// List all keys
pub fn list(conn: &Connection, page: u32, limit: u32) -> Result<(Vec<ApiKey>, i64)> {
    let offset = (page.saturating_sub(1)) * limit;

    let total: i64 = conn.query_row("SELECT COUNT(*) FROM api_keys", [], |row| row.get(0))
        .unwrap_or(0);

    let mut stmt = conn.prepare(
        "SELECT id, key_hash, name, is_admin, rate_limit_rps, request_count, last_used_at, created_at
         FROM api_keys ORDER BY created_at DESC LIMIT ?1 OFFSET ?2"
    ).map_err(|e| GscholarError::Database(format!("Prepare failed: {}", e)))?;

    let rows = stmt.query_map(params![limit, offset], |row| {
        Ok(ApiKey {
            id: row.get(0)?,
            key_hash: row.get(1)?,
            name: row.get(2)?,
            is_admin: row.get::<_, i32>(3)? != 0,
            rate_limit_rps: row.get::<_, i32>(4)? as u32,
            request_count: row.get(5)?,
            last_used_at: row.get(6)?,
            created_at: row.get(7)?,
        })
    }).map_err(|e| GscholarError::Database(format!("Query failed: {}", e)))?;

    let keys: Vec<ApiKey> = rows.filter_map(|r| r.ok()).collect();
    Ok((keys, total))
}

/// Update key settings
pub fn update(conn: &Connection, id: &str, name: Option<&str>, rate_limit_rps: Option<u32>) -> Result<bool> {
    let mut updates = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(n) = name {
        updates.push("name = ?");
        values.push(Box::new(n.to_string()));
    }
    if let Some(r) = rate_limit_rps {
        updates.push("rate_limit_rps = ?");
        values.push(Box::new(r as i32));
    }

    if updates.is_empty() {
        return Ok(false);
    }

    values.push(Box::new(id.to_string()));
    let sql = format!(
        "UPDATE api_keys SET {} WHERE id = ?",
        updates.join(", ")
    );

    let params: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    let rows = conn.execute(&sql, params.as_slice())
        .map_err(|e| GscholarError::Database(format!("Update failed: {}", e)))?;

    Ok(rows > 0)
}

/// Delete a key
pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    let rows = conn.execute("DELETE FROM api_keys WHERE id = ?1", params![id])
        .map_err(|e| GscholarError::Database(format!("Delete failed: {}", e)))?;
    
    Ok(rows > 0)
}

/// Check if any admin key exists
pub fn has_admin_key(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM api_keys WHERE is_admin = 1",
        [],
        |row| row.get(0),
    ).unwrap_or(0);

    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::init_tables;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        init_tables(&conn).expect("init tables");
        conn
    }

    #[test]
    fn test_create_and_validate() {
        let conn = setup_db();
        
        let created = create(&conn, "Test Key", false, 10).expect("create");
        assert!(created.key.expose_secret().starts_with("rgs_"));

        let validated = validate(&conn, created.key.expose_secret()).expect("validate");
        assert!(validated.is_some());
        let key = validated.expect("key");
        assert_eq!(key.name, "Test Key");
        assert!(!key.is_admin);
    }

    #[test]
    fn test_invalid_key() {
        let conn = setup_db();
        
        let validated = validate(&conn, "invalid-key").expect("validate");
        assert!(validated.is_none());
    }

    #[test]
    fn test_constant_time_verify() {
        let (key, hash) = generate_key();
        
        // Correct key should verify
        assert!(verify_key_hash(key.expose_secret(), &hash));
        
        // Wrong key should not verify
        assert!(!verify_key_hash("wrong-key", &hash));
    }

    #[test]
    fn test_record_usage() {
        let conn = setup_db();
        
        let created = create(&conn, "Test", false, 10).expect("create");
        let hash = hash_key(created.key.expose_secret());
        
        record_usage(&conn, &hash).expect("record");
        record_usage(&conn, &hash).expect("record");

        let key = get_by_id(&conn, &created.id).expect("get").expect("found");
        assert_eq!(key.request_count, 2);
    }

    #[test]
    fn test_list_keys() {
        let conn = setup_db();
        
        create(&conn, "Key 1", false, 10).expect("create");
        create(&conn, "Key 2", true, 20).expect("create");

        let (keys, total) = list(&conn, 1, 10).expect("list");
        assert_eq!(keys.len(), 2);
        assert_eq!(total, 2);
    }

    #[test]
    fn test_has_admin_key() {
        let conn = setup_db();
        
        assert!(!has_admin_key(&conn).expect("check"));
        
        create(&conn, "Admin", true, 100).expect("create");
        assert!(has_admin_key(&conn).expect("check"));
    }
    
    #[test]
    fn test_key_not_logged() {
        let created = ApiKeyCreated {
            id: "test".to_string(),
            key: SecretString::from("secret-key".to_string()),
            name: "Test".to_string(),
            is_admin: false,
            rate_limit_rps: 10,
            created_at: 0,
        };
        
        let debug_output = format!("{:?}", created);
        assert!(debug_output.contains("[REDACTED]"));
        assert!(!debug_output.contains("secret-key"));
    }
}
