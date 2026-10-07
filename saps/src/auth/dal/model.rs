use crate::auth::dal::tx_definitions::CompareAndSwapAuthSessionMeta;
use crate::auth::token::checks::UserRole;
use crate::dal::connections::{AuthPostGresDescriptor, YieldPostGresPool};
use crate::errors::saps::SapsError;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sqlx::{Executor, Pool, Postgres, Row, postgres::PgRow};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct AuthSession<U: UserRole> {
    /// The unique identifier of the session.
    pub id: Uuid,
    /// The role of the user for this session.
    pub role: U,
    /// The timestamp when the session was created.
    pub date_created: NaiveDateTime,
    /// The timestamp when the session was last interacted with.
    pub last_interacted: NaiveDateTime,
    /// Optional JSON metadata attached to the session.
    pub meta: Option<serde_json::Value>,
}

impl<U: UserRole> AuthSession<U> {
    /// Constructs an `AuthSession` from a Postgres row, converting the `role`
    /// column (VARCHAR) into `U` via `TryFrom<String>`.
    pub fn from_row(row: &PgRow) -> Result<Self, SapsError> {
        let role_str: String = row
            .try_get("role")
            .map_err(|e| SapsError::unknown(e.to_string()))?;
        let role = U::try_from(role_str)?;
        Ok(Self {
            id: row
                .try_get("id")
                .map_err(|e| SapsError::unknown(e.to_string()))?,
            role,
            date_created: row
                .try_get("date_created")
                .map_err(|e| SapsError::unknown(e.to_string()))?,
            last_interacted: row
                .try_get("last_interacted")
                .map_err(|e| SapsError::unknown(e.to_string()))?,
            meta: row
                .try_get("meta")
                .map_err(|e| SapsError::unknown(e.to_string()))?,
        })
    }
}

impl<U: UserRole> AuthSession<U> {
    /// Creates a new `AuthSession` with a random UUID, the current timestamp, and `meta` set to `None`.
    pub fn new(role: U) -> Self {
        let now = chrono::Utc::now().naive_utc();
        Self {
            id: Uuid::new_v4(),
            role,
            date_created: now,
            last_interacted: now,
            meta: None,
        }
    }

    /// Attaches JSON metadata to the session. Accepts any type that implements `Serialize`.
    pub fn with_meta<M: Serialize>(mut self, meta: M) -> Self {
        self.meta = Some(serde_json::to_value(meta).expect("failed to serialize meta to JSON"));
        self
    }

    /// Returns a reference to the JSON value at top-level `key` in `meta`,
    /// or `None` if `meta` is `NULL` or does not contain `key`.
    ///
    /// Use [`meta_get_owned`](Self::meta_get_owned) if you need an owned clone.
    pub fn meta_get(&self, key: &str) -> Option<&serde_json::Value> {
        self.meta.as_ref().and_then(|m| m.get(key))
    }

    /// Owned counterpart of [`meta_get`](Self::meta_get) — clones the value.
    pub fn meta_get_owned(&self, key: &str) -> Option<serde_json::Value> {
        self.meta_get(key).cloned()
    }

    /// Returns a reference to the JSON value at top-level `key` in `meta`, or
    /// `SapsError::not_found(...)` if `meta` is `NULL` or does not contain `key`.
    ///
    /// Use [`meta_get_strict_owned`](Self::meta_get_strict_owned) if you need
    /// an owned clone.
    pub fn meta_get_strict(&self, key: &str) -> Result<&serde_json::Value, SapsError> {
        self.meta_get(key)
            .ok_or_else(|| SapsError::not_found(format!("meta key not found: {}", key)))
    }

    /// Owned counterpart of [`meta_get_strict`](Self::meta_get_strict) — clones the value.
    pub fn meta_get_strict_owned(&self, key: &str) -> Result<serde_json::Value, SapsError> {
        self.meta_get_strict(key).map(Clone::clone)
    }

    /// Returns the value at top-level `key` in `meta` deserialized as `T`,
    /// borrowing from `meta` where possible.
    ///
    /// Because `T: Deserialize<'a>` is bounded by the borrow of `self`, types
    /// like `&str` deserialize without allocating (the returned reference
    /// borrows from the underlying `Value`'s storage). Owning types like
    /// `String` or `i32` continue to work and just copy as usual.
    ///
    /// Returns `Ok(None)` if `meta` is `NULL` or does not contain `key`.
    /// Returns `Err(SapsError::bad_request(...))` if the value is present but
    /// cannot be decoded as `T`.
    pub fn meta_get_typed<'a, T>(&'a self, key: &str) -> Result<Option<T>, SapsError>
    where
        T: Deserialize<'a>,
    {
        match self.meta_get(key) {
            Some(value) => T::deserialize(value).map(Some).map_err(|e| {
                SapsError::bad_request(format!(
                    "failed to deserialize meta key {}: {}",
                    key, e
                ))
            }),
            None => Ok(None),
        }
    }

    /// Owned counterpart of [`meta_get_typed`](Self::meta_get_typed) —
    /// requires `T: DeserializeOwned` and works through a cloned `Value`.
    pub fn meta_get_typed_owned<T: DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<Option<T>, SapsError> {
        match self.meta_get_owned(key) {
            Some(value) => serde_json::from_value(value).map(Some).map_err(|e| {
                SapsError::bad_request(format!(
                    "failed to deserialize meta key {}: {}",
                    key, e
                ))
            }),
            None => Ok(None),
        }
    }

    /// Returns the value at top-level `key` in `meta` deserialized as `T`,
    /// borrowing from `meta` where possible, or `SapsError::not_found(...)`
    /// if the key is missing.
    ///
    /// Returns `Err(SapsError::bad_request(...))` if the value is present but
    /// cannot be decoded as `T`.
    pub fn meta_get_typed_strict<'a, T>(&'a self, key: &str) -> Result<T, SapsError>
    where
        T: Deserialize<'a>,
    {
        let value = self.meta_get_strict(key)?;
        T::deserialize(value).map_err(|e| {
            SapsError::bad_request(format!("failed to deserialize meta key {}: {}", key, e))
        })
    }

    /// Owned counterpart of
    /// [`meta_get_typed_strict`](Self::meta_get_typed_strict) — requires
    /// `T: DeserializeOwned` and works through a cloned `Value`.
    pub fn meta_get_typed_strict_owned<T: DeserializeOwned>(
        &self,
        key: &str,
    ) -> Result<T, SapsError> {
        let value = self.meta_get_strict_owned(key)?;
        serde_json::from_value(value).map_err(|e| {
            SapsError::bad_request(format!("failed to deserialize meta key {}: {}", key, e))
        })
    }

    /// Atomic compare-and-swap on a single top-level key in `meta` for this
    /// session, against the pool exposed by `Y`.
    ///
    /// Convenience wrapper around
    /// [`AuthPostGresDescriptor::<Y>::compare_and_swap_auth_session_meta`]
    /// using `self.id` as the session id.
    ///
    /// Returns `Some(updated_session)` if the swap happened, or `None` if
    /// the session no longer exists, the key is absent, or its value does
    /// not equal `expected`. See the trait docs for the exact match
    /// semantics.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if the underlying query fails.
    pub async fn compare_and_swap_meta<Y: YieldPostGresPool>(
        &self,
        key: &str,
        expected: serde_json::Value,
        new_value: serde_json::Value,
    ) -> Result<Option<AuthSession<U>>, sqlx::Error> {
        AuthPostGresDescriptor::<Y>::compare_and_swap_auth_session_meta::<U>(
            &self.id.to_string(),
            key,
            expected,
            new_value,
        )
        .await
    }

    /// Returns a SQL script that creates the `saps` schema (if it doesn't exist)
    /// and the `saps.auth_sessions` table (if it doesn't exist) matching this struct's fields.
    pub fn generate_migration_sql() -> &'static str {
        r#"
CREATE SCHEMA IF NOT EXISTS saps;

CREATE TABLE IF NOT EXISTS saps.auth_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    old_id UUID NOT NULL DEFAULT gen_random_uuid(),
    role VARCHAR(255) NOT NULL,
    date_created TIMESTAMP NOT NULL DEFAULT NOW(),
    last_interacted TIMESTAMP NOT NULL DEFAULT NOW(),
    time_minted TIMESTAMP NOT NULL DEFAULT NOW(),
    meta JSONB
);

ALTER TABLE saps.auth_sessions
    ADD COLUMN IF NOT EXISTS old_id UUID;
UPDATE saps.auth_sessions SET old_id = id WHERE old_id IS NULL;
ALTER TABLE saps.auth_sessions ALTER COLUMN old_id SET NOT NULL;
ALTER TABLE saps.auth_sessions ALTER COLUMN old_id SET DEFAULT gen_random_uuid();

ALTER TABLE saps.auth_sessions
    ADD COLUMN IF NOT EXISTS time_minted TIMESTAMP NOT NULL DEFAULT NOW();

CREATE INDEX IF NOT EXISTS idx_saps_auth_sessions_last_interacted
    ON saps.auth_sessions (last_interacted);

CREATE INDEX IF NOT EXISTS idx_saps_auth_sessions_old_id
    ON saps.auth_sessions (old_id);

CREATE OR REPLACE FUNCTION saps.ping(
    p_minutes INTEGER,
    p_session_id UUID
)
RETURNS saps.auth_sessions
LANGUAGE plpgsql
AS $$
DECLARE
    session_record saps.auth_sessions;
    rows_affected INTEGER;
BEGIN
    DELETE FROM saps.auth_sessions
    WHERE (id = p_session_id OR old_id = p_session_id)
      AND last_interacted < NOW() - (p_minutes || ' minutes')::INTERVAL;

    GET DIAGNOSTICS rows_affected = ROW_COUNT;

    IF rows_affected > 0 THEN
        RETURN NULL;
    END IF;

    SELECT * INTO session_record
    FROM saps.auth_sessions
    WHERE id = p_session_id OR old_id = p_session_id
    LIMIT 1;

    IF NOT FOUND THEN
        RETURN NULL;
    END IF;

    -- Grace period: caller used the previous id of an already-rotated session.
    -- Accept the ping if rotation happened within the last 10 seconds, but do
    -- not rotate again and do not refresh time_minted.
    IF session_record.id <> p_session_id AND session_record.old_id = p_session_id THEN
        IF session_record.time_minted < NOW() - INTERVAL '10 seconds' THEN
            RETURN NULL;
        END IF;

        UPDATE saps.auth_sessions
        SET last_interacted = NOW()
        WHERE id = session_record.id
        RETURNING * INTO session_record;

        RETURN session_record;
    END IF;

    -- Caller used the current id and the session is old enough to rotate.
    IF session_record.date_created < NOW() - INTERVAL '5 minutes' THEN
        UPDATE saps.auth_sessions
        SET old_id = id,
            id = gen_random_uuid(),
            date_created = NOW(),
            last_interacted = NOW(),
            time_minted = NOW()
        WHERE id = p_session_id
        RETURNING * INTO session_record;

        RETURN session_record;
    END IF;

    UPDATE saps.auth_sessions
    SET last_interacted = NOW()
    WHERE id = p_session_id
    RETURNING * INTO session_record;

    RETURN session_record;
END;
$$;
"#
    }

    /// Returns SQL that creates a partial unique index enforcing that no two
    /// sessions share the same value at `meta->>key`.
    ///
    /// Run this once per `meta` key whose value should be unique across all
    /// sessions (e.g. `"user_id"` to enforce one active session per user).
    /// Sessions whose `meta` does not contain `key` (including those where
    /// `meta IS NULL`) are excluded by the partial `WHERE meta ? key` clause,
    /// so they never collide with one another.
    ///
    /// The index name is `idx_saps_auth_sessions_meta_<key>` with non-alnum
    /// characters stripped, so two keys that differ only in punctuation would
    /// collide. Pass simple alphanumeric keys.
    ///
    /// # Panics
    ///
    /// Panics if `key` is empty, or contains a single quote (which would
    /// break the embedded SQL string literal).
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Run after generate_migration_sql to enforce one session per user_id
    /// sqlx::raw_sql(AuthSession::<MyRole>::generate_migration_sql())
    ///     .execute(&pool).await?;
    /// sqlx::raw_sql(sqlx::AssertSqlSafe(AuthSession::<MyRole>::generate_unique_meta_key_sql("user_id")))
    ///     .execute(&pool).await?;
    /// ```
    pub fn generate_unique_meta_key_sql(key: &str) -> String {
        assert!(!key.is_empty(), "meta key must not be empty");
        assert!(
            !key.contains('\''),
            "meta key must not contain single quotes: {:?}",
            key
        );
        let safe_name: String = key
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        format!(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_saps_auth_sessions_meta_{safe_name} \
             ON saps.auth_sessions ((meta->>'{key}')) \
             WHERE meta ? '{key}';\n",
        )
    }

    /// Returns SQL that creates a partial unique index enforcing that no two
    /// sessions share the same `(meta->>key1, meta->>key2)` pair.
    ///
    /// Use this when uniqueness needs to be scoped to a combination of two
    /// meta keys rather than a single value — e.g. `("user_id", "server_tag")`
    /// so the same user can hold one active session per server (app server,
    /// metrics server, etc.) without colliding with their session on a
    /// different server. Sessions missing either key are excluded by the
    /// partial `WHERE meta ? key1 AND meta ? key2` clause and never collide.
    ///
    /// The index name is `idx_saps_auth_sessions_meta_<key1>_<key2>` with
    /// non-alnum characters in each key stripped, so two key pairs that
    /// differ only in punctuation would collide. Pass simple alphanumeric
    /// keys.
    ///
    /// # Panics
    ///
    /// Panics if either key is empty or contains a single quote (which would
    /// break the embedded SQL string literal).
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Allow one session per (user_id, server_tag) pair
    /// sqlx::raw_sql(sqlx::AssertSqlSafe(AuthSession::<MyRole>::generate_unique_meta_key_pair_sql(
    ///     "user_id", "server_tag",
    /// )))
    /// .execute(&pool).await?;
    /// ```
    pub fn generate_unique_meta_key_pair_sql(key1: &str, key2: &str) -> String {
        assert!(!key1.is_empty(), "first meta key must not be empty");
        assert!(!key2.is_empty(), "second meta key must not be empty");
        assert!(
            !key1.contains('\''),
            "first meta key must not contain single quotes: {:?}",
            key1
        );
        assert!(
            !key2.contains('\''),
            "second meta key must not contain single quotes: {:?}",
            key2
        );
        let safe = |k: &str| -> String {
            k.chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect()
        };
        let safe1 = safe(key1);
        let safe2 = safe(key2);
        format!(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_saps_auth_sessions_meta_{safe1}_{safe2} \
             ON saps.auth_sessions ((meta->>'{key1}'), (meta->>'{key2}')) \
             WHERE meta ? '{key1}' AND meta ? '{key2}';\n",
        )
    }

    /// Runs the auth session migration against the pool exposed by `Y`.
    ///
    /// Executes [`generate_migration_sql`](Self::generate_migration_sql) and
    /// then one [`generate_unique_meta_key_sql`](Self::generate_unique_meta_key_sql)
    /// statement for every entry in `unique_meta_keys`. Pass `&[]` to skip the
    /// uniqueness indexes. Each statement uses `IF NOT EXISTS` / `CREATE OR
    /// REPLACE`, so this is safe to call on every startup.
    ///
    /// Thin wrapper around
    /// [`run_migration_with_pool`](Self::run_migration_with_pool) — use that
    /// directly if you already hold a `&Pool<Postgres>`.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if any statement fails (e.g. an existing unique
    /// index would be violated by current data).
    pub async fn run_migration<Y: YieldPostGresPool>(
        unique_meta_keys: &[&str],
    ) -> Result<(), sqlx::Error> {
        Self::run_migration_with_pool(Y::yield_pool(), unique_meta_keys).await
    }

    /// Same as [`run_migration`](Self::run_migration) but takes the pool by
    /// reference instead of via the [`YieldPostGresPool`] trait.
    ///
    /// Useful when the caller already has a `&Pool<Postgres>` (e.g. inside a
    /// startup routine or a one-off script) and doesn't want to wire up a
    /// `YieldPostGresPool` impl just to run the migration.
    ///
    /// # Errors
    ///
    /// Returns `sqlx::Error` if any statement fails (e.g. an existing unique
    /// index would be violated by current data).
    pub async fn run_migration_with_pool(
        pool: &Pool<Postgres>,
        unique_meta_keys: &[&str],
    ) -> Result<(), sqlx::Error> {
        pool.execute(Self::generate_migration_sql()).await?;
        for key in unique_meta_keys {
            let sql = Self::generate_unique_meta_key_sql(key);
            // safe: the generator rejects quotes in `key` and sanitises the index name
            pool.execute(sqlx::AssertSqlSafe(sql)).await?;
        }
        Ok(())
    }
}

// MARK: - Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::dal::tx_definitions::{
        CreateAuthSession, DeleteAuthSession, DeleteAuthSessionMetaKey,
        DeleteAuthSessionsByMetaKey, DeleteAuthSessionsByMetaKeyPair, GetAllAuthSessions,
        GetAuthSession, GetAuthSessionByMetaKey,
        GetAuthSessionByMetaKeyStrict, GetAuthSessionStrict, GetAuthSessionsByMetaKey,
        GetAuthSessionsByMetaKeyPair, PingAuthSession, UpsertAuthSessionMetaKey,
        UpsertAuthSessionsMetaKeyByMeta, UpsertAuthSessionsMetaKeyByMetaKeyPair,
    };
    use crate::errors::saps::SapsErrorStatus;
    use crate::dal::connections::AuthPostGresDescriptor;

    #[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
    enum TestRole {
        Admin,
        Customer,
    }

    impl std::fmt::Display for TestRole {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                TestRole::Admin => write!(f, "admin"),
                TestRole::Customer => write!(f, "customer"),
            }
        }
    }

    impl TryFrom<String> for TestRole {
        type Error = SapsError;
        fn try_from(value: String) -> Result<Self, Self::Error> {
            match value.to_lowercase().as_str() {
                "admin" => Ok(TestRole::Admin),
                "customer" => Ok(TestRole::Customer),
                _ => Err(SapsError::bad_request(format!("Unknown role: {}", value))),
            }
        }
    }

    impl crate::auth::token::checks::UserRole for TestRole {}

    #[saps::db_test]
    async fn test_create_auth_session() {
        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await
            .expect("failed to get all sessions");
        assert_eq!(all.len(), 0);

        let session = AuthSession::new(TestRole::Admin);
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create auth session");
        assert_eq!(created.role, TestRole::Admin);
        assert!(created.meta.is_none());

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await
            .expect("failed to get all sessions");
        assert_eq!(all.len(), 1);
    }

    #[saps::db_test]
    async fn test_create_auth_session_with_meta() {
        let meta = serde_json::json!({"user_id": 2, "department": "engineering", "level": 3});
        let session = AuthSession::new(TestRole::Customer).with_meta(meta.clone());
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create auth session with meta");
        assert_eq!(created.role, TestRole::Customer);
        assert_eq!(created.meta, Some(meta));
    }

    #[saps::db_test]
    async fn test_ping_active_session() {
        let session = AuthSession::new(TestRole::Admin);
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");

        let pinged = AuthPostGresDescriptor::<TestDbHandle>::ping_auth_session::<TestRole>(
            30,
            &created.id.to_string(),
        )
        .await
        .expect("failed to ping session");
        assert!(pinged.is_some());
        let pinged = pinged.unwrap();
        assert_eq!(pinged.role, TestRole::Admin);
    }

    #[saps::db_test]
    async fn test_ping_nonexistent_session_returns_none() {
        let fake_id = uuid::Uuid::new_v4().to_string();
        let pinged =
            AuthPostGresDescriptor::<TestDbHandle>::ping_auth_session::<TestRole>(30, &fake_id)
                .await
                .expect("failed to ping session");
        assert!(pinged.is_none());
    }

    #[saps::db_test]
    async fn test_ping_expired_session_returns_none() {
        let session =
            AuthSession::new(TestRole::Customer).with_meta(serde_json::json!({"user_id": 4}));
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await
            .expect("failed to get all sessions");
        assert_eq!(all.len(), 1);

        // Manually backdate last_interacted so the session is expired
        saps::sqlx::query(
            "UPDATE saps.auth_sessions SET last_interacted = NOW() - INTERVAL '2 hours' WHERE id = $1"
        )
            .bind(created.id)
            .execute(pool)
            .await
            .expect("failed to backdate session");

        // Ping with a 30-minute timeout — session should be expired and deleted
        let pinged = AuthPostGresDescriptor::<TestDbHandle>::ping_auth_session::<TestRole>(
            30,
            &created.id.to_string(),
        )
        .await
        .expect("failed to ping session");
        assert!(pinged.is_none());

        // Expired session should have been deleted by ping
        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await
            .expect("failed to get all sessions");
        assert_eq!(all.len(), 0);
    }

    #[saps::db_test]
    async fn test_delete_auth_session() {
        let session =
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 5}));
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await
            .expect("failed to get all sessions");
        assert_eq!(all.len(), 1);

        let deleted = AuthPostGresDescriptor::<TestDbHandle>::delete_auth_session(created.id)
            .await
            .expect("failed to delete session");
        assert!(deleted);

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await
            .expect("failed to get all sessions");
        assert_eq!(all.len(), 0);
    }

    #[saps::db_test]
    async fn test_delete_nonexistent_session_returns_false() {
        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await
            .expect("failed to get all sessions");
        assert_eq!(all.len(), 0);

        let fake_id = uuid::Uuid::new_v4();
        let deleted = AuthPostGresDescriptor::<TestDbHandle>::delete_auth_session(fake_id)
            .await
            .expect("failed to delete session");
        assert!(!deleted);

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await
            .expect("failed to get all sessions");
        assert_eq!(all.len(), 0);
    }

    #[saps::db_test]
    async fn test_create_and_ping_preserves_meta() {
        let meta = serde_json::json!({"user_id": 6, "team": "backend"});
        let session = AuthSession::new(TestRole::Admin).with_meta(meta.clone());
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");

        let pinged = AuthPostGresDescriptor::<TestDbHandle>::ping_auth_session::<TestRole>(
            30,
            &created.id.to_string(),
        )
        .await
        .expect("failed to ping session");
        let pinged = pinged.expect("session should exist");
        assert_eq!(pinged.meta, Some(meta));
    }

    #[saps::db_test]
    async fn test_upsert_meta_key_on_null_meta() {
        let session = AuthSession::new(TestRole::Admin);
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");
        assert!(created.meta.is_none());

        let updated =
            AuthPostGresDescriptor::<TestDbHandle>::upsert_auth_session_meta_key::<TestRole>(
                &created.id.to_string(),
                "user_id",
                serde_json::json!(42),
            )
            .await
            .expect("failed to upsert meta key")
            .expect("session should exist");
        assert_eq!(updated.id, created.id);
        assert_eq!(updated.role, TestRole::Admin);
        assert_eq!(updated.meta, Some(serde_json::json!({"user_id": 42})));
    }

    #[saps::db_test]
    async fn test_upsert_meta_key_preserves_existing_keys() {
        let meta = serde_json::json!({"user_id": 7, "team": "backend"});
        let session = AuthSession::new(TestRole::Customer).with_meta(meta);
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");

        let updated =
            AuthPostGresDescriptor::<TestDbHandle>::upsert_auth_session_meta_key::<TestRole>(
                &created.id.to_string(),
                "level",
                serde_json::json!(3),
            )
            .await
            .expect("failed to upsert meta key")
            .expect("session should exist");
        assert_eq!(updated.id, created.id);
        assert_eq!(
            updated.meta,
            Some(serde_json::json!({"user_id": 7, "team": "backend", "level": 3}))
        );
    }

    #[saps::db_test]
    async fn test_upsert_meta_key_overwrites_existing_key() {
        let meta = serde_json::json!({"user_id": 7, "team": "backend"});
        let session = AuthSession::new(TestRole::Admin).with_meta(meta);
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");

        let updated =
            AuthPostGresDescriptor::<TestDbHandle>::upsert_auth_session_meta_key::<TestRole>(
                &created.id.to_string(),
                "team",
                serde_json::json!("platform"),
            )
            .await
            .expect("failed to upsert meta key")
            .expect("session should exist");
        assert_eq!(
            updated.meta,
            Some(serde_json::json!({"user_id": 7, "team": "platform"}))
        );
    }

    #[saps::db_test]
    async fn test_upsert_meta_key_missing_session_returns_none() {
        let fake_id = uuid::Uuid::new_v4().to_string();
        let result =
            AuthPostGresDescriptor::<TestDbHandle>::upsert_auth_session_meta_key::<TestRole>(
                &fake_id,
                "user_id",
                serde_json::json!(1),
            )
            .await
            .expect("failed to upsert meta key");
        assert!(result.is_none());
    }

    #[saps::db_test]
    async fn test_delete_meta_key_preserves_other_keys() {
        let meta = serde_json::json!({"user_id": 7, "team": "backend", "level": 3});
        let session = AuthSession::new(TestRole::Admin).with_meta(meta);
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");

        let updated =
            AuthPostGresDescriptor::<TestDbHandle>::delete_auth_session_meta_key::<TestRole>(
                &created.id.to_string(),
                "team",
            )
            .await
            .expect("failed to delete meta key")
            .expect("session should exist");
        assert_eq!(updated.id, created.id);
        assert_eq!(
            updated.meta,
            Some(serde_json::json!({"user_id": 7, "level": 3}))
        );
    }

    #[saps::db_test]
    async fn test_delete_missing_meta_key_is_noop() {
        let meta = serde_json::json!({"user_id": 7});
        let session = AuthSession::new(TestRole::Customer).with_meta(meta.clone());
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");

        let updated =
            AuthPostGresDescriptor::<TestDbHandle>::delete_auth_session_meta_key::<TestRole>(
                &created.id.to_string(),
                "does_not_exist",
            )
            .await
            .expect("failed to delete meta key")
            .expect("session should exist");
        assert_eq!(updated.meta, Some(meta));
    }

    #[saps::db_test]
    async fn test_delete_meta_key_on_null_meta_stays_null() {
        let session = AuthSession::new(TestRole::Admin);
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");
        assert!(created.meta.is_none());

        let updated =
            AuthPostGresDescriptor::<TestDbHandle>::delete_auth_session_meta_key::<TestRole>(
                &created.id.to_string(),
                "anything",
            )
            .await
            .expect("failed to delete meta key")
            .expect("session should exist");
        assert!(updated.meta.is_none());
    }

    #[saps::db_test]
    async fn test_delete_meta_key_missing_session_returns_none() {
        let fake_id = uuid::Uuid::new_v4().to_string();
        let result =
            AuthPostGresDescriptor::<TestDbHandle>::delete_auth_session_meta_key::<TestRole>(
                &fake_id,
                "anything",
            )
            .await
            .expect("failed to delete meta key");
        assert!(result.is_none());
    }

    #[saps::db_test]
    async fn test_get_auth_sessions_by_meta_key_filters_matches() {
        let _a = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create a");
        let _b = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create b");
        let _c = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 2})),
        )
        .await
        .expect("create c");

        let matches =
            AuthPostGresDescriptor::<TestDbHandle>::get_auth_sessions_by_meta_key::<TestRole>(
                "user_id",
                serde_json::json!(1),
            )
            .await
            .expect("get by meta key");
        assert_eq!(matches.len(), 2);
        for session in &matches {
            assert_eq!(session.meta.as_ref().unwrap()["user_id"], serde_json::json!(1));
        }
    }

    #[saps::db_test]
    async fn test_get_auth_sessions_by_meta_key_no_matches() {
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create");

        let matches =
            AuthPostGresDescriptor::<TestDbHandle>::get_auth_sessions_by_meta_key::<TestRole>(
                "user_id",
                serde_json::json!(99),
            )
            .await
            .expect("get by meta key");
        assert!(matches.is_empty());
    }

    #[saps::db_test]
    async fn test_get_auth_sessions_by_meta_key_ignores_null_meta() {
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(AuthSession::new(
            TestRole::Admin,
        ))
        .await
        .expect("create");

        let matches =
            AuthPostGresDescriptor::<TestDbHandle>::get_auth_sessions_by_meta_key::<TestRole>(
                "user_id",
                serde_json::json!(1),
            )
            .await
            .expect("get by meta key");
        assert!(matches.is_empty());
    }

    #[saps::db_test]
    async fn test_get_auth_session_by_meta_key_returns_one() {
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer).with_meta(serde_json::json!({"user_id": 7})),
        )
        .await
        .expect("create");

        let found =
            AuthPostGresDescriptor::<TestDbHandle>::get_auth_session_by_meta_key::<TestRole>(
                "user_id",
                serde_json::json!(7),
            )
            .await
            .expect("get by meta key")
            .expect("session should exist");
        assert_eq!(found.id, created.id);
        assert_eq!(found.role, TestRole::Customer);
    }

    #[saps::db_test]
    async fn test_get_auth_session_by_meta_key_no_match_returns_none() {
        let result =
            AuthPostGresDescriptor::<TestDbHandle>::get_auth_session_by_meta_key::<TestRole>(
                "user_id",
                serde_json::json!(7),
            )
            .await
            .expect("get by meta key");
        assert!(result.is_none());
    }

    #[saps::db_test]
    async fn test_get_auth_sessions_by_meta_key_pair_filters_to_pair() {
        let target = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "app"})),
        )
        .await
        .expect("create target");
        // Same user, different server — must NOT match the pair lookup.
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "metrics"})),
        )
        .await
        .expect("create other server");
        // Different user, same server — must NOT match either.
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer)
                .with_meta(serde_json::json!({"user_id": 2, "server_tag": "app"})),
        )
        .await
        .expect("create other user");
        // Only one of the two keys present — must NOT match.
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create with only user_id");

        let matches =
            AuthPostGresDescriptor::<TestDbHandle>::get_auth_sessions_by_meta_key_pair::<TestRole>(
                "user_id",
                serde_json::json!(1),
                "server_tag",
                serde_json::json!("app"),
            )
            .await
            .expect("get by meta key pair");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, target.id);
    }

    #[saps::db_test]
    async fn test_get_auth_sessions_by_meta_key_pair_no_match_returns_empty() {
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "app"})),
        )
        .await
        .expect("create");

        let matches =
            AuthPostGresDescriptor::<TestDbHandle>::get_auth_sessions_by_meta_key_pair::<TestRole>(
                "user_id",
                serde_json::json!(1),
                "server_tag",
                serde_json::json!("metrics"),
            )
            .await
            .expect("get by meta key pair");
        assert!(matches.is_empty());
    }

    #[saps::db_test]
    async fn test_get_auth_sessions_by_meta_key_pair_ignores_null_meta() {
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(AuthSession::new(
            TestRole::Admin,
        ))
        .await
        .expect("create without meta");

        let matches =
            AuthPostGresDescriptor::<TestDbHandle>::get_auth_sessions_by_meta_key_pair::<TestRole>(
                "user_id",
                serde_json::json!(1),
                "server_tag",
                serde_json::json!("app"),
            )
            .await
            .expect("get by meta key pair");
        assert!(matches.is_empty());
    }

    #[saps::db_test]
    async fn test_delete_auth_sessions_by_meta_key() {
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create a");
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create b");
        let survivor = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 2})),
        )
        .await
        .expect("create c");

        let deleted =
            AuthPostGresDescriptor::<TestDbHandle>::delete_auth_sessions_by_meta_key(
                "user_id",
                serde_json::json!(1),
            )
            .await
            .expect("delete by meta key");
        assert_eq!(deleted, 2);

        let remaining =
            AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
                .await
                .expect("get all");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, survivor.id);
    }

    #[saps::db_test]
    async fn test_delete_auth_sessions_by_meta_key_pair_only_deletes_full_pair() {
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "app"})),
        )
        .await
        .expect("create target");
        // Same user, different server — must survive.
        let other_server = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "metrics"})),
        )
        .await
        .expect("create other server");
        // Different user, same server — must survive.
        let other_user = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer)
                .with_meta(serde_json::json!({"user_id": 2, "server_tag": "app"})),
        )
        .await
        .expect("create other user");
        // Only one of the two keys present — must survive.
        let partial = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create with only user_id");

        let deleted =
            AuthPostGresDescriptor::<TestDbHandle>::delete_auth_sessions_by_meta_key_pair(
                "user_id",
                serde_json::json!(1),
                "server_tag",
                serde_json::json!("app"),
            )
            .await
            .expect("delete by meta key pair");
        assert_eq!(deleted, 1);

        let remaining =
            AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
                .await
                .expect("get all");
        let ids: Vec<_> = remaining.iter().map(|s| s.id).collect();
        assert_eq!(remaining.len(), 3);
        assert!(ids.contains(&other_server.id));
        assert!(ids.contains(&other_user.id));
        assert!(ids.contains(&partial.id));
    }

    #[saps::db_test]
    async fn test_delete_auth_sessions_by_meta_key_pair_no_matches() {
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "app"})),
        )
        .await
        .expect("create");

        let deleted =
            AuthPostGresDescriptor::<TestDbHandle>::delete_auth_sessions_by_meta_key_pair(
                "user_id",
                serde_json::json!(1),
                "server_tag",
                serde_json::json!("metrics"),
            )
            .await
            .expect("delete by meta key pair");
        assert_eq!(deleted, 0);

        let remaining =
            AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
                .await
                .expect("get all");
        assert_eq!(remaining.len(), 1);
    }

    #[saps::db_test]
    async fn test_delete_auth_sessions_by_meta_key_no_matches() {
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create");

        let deleted =
            AuthPostGresDescriptor::<TestDbHandle>::delete_auth_sessions_by_meta_key(
                "user_id",
                serde_json::json!(99),
            )
            .await
            .expect("delete by meta key");
        assert_eq!(deleted, 0);

        let remaining =
            AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
                .await
                .expect("get all");
        assert_eq!(remaining.len(), 1);
    }

    #[saps::db_test]
    async fn test_unique_meta_key_blocks_duplicate_value() {
        AuthSession::<TestRole>::run_migration::<TestDbHandle>(&["user_id"])
            .await
            .expect("install unique meta key index");

        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("first insert should succeed");

        let dup = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await;
        let err = dup.expect_err("second insert with duplicate user_id must fail");
        let db_err = match err {
            sqlx::Error::Database(db_err) => db_err,
            other => panic!("expected database error, got {:?}", other),
        };
        assert!(
            db_err.is_unique_violation(),
            "expected unique violation, got: {}",
            db_err
        );
    }

    #[saps::db_test]
    async fn test_unique_meta_key_allows_distinct_values() {
        AuthSession::<TestRole>::run_migration::<TestDbHandle>(&["user_id"])
            .await
            .expect("install unique meta key index");

        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("first insert");
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer).with_meta(serde_json::json!({"user_id": 2})),
        )
        .await
        .expect("second insert with different user_id");

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await
            .expect("get all");
        assert_eq!(all.len(), 2);
    }

    #[saps::db_test]
    async fn test_unique_meta_key_pair_blocks_duplicate_pair() {
        AuthSession::<TestRole>::run_migration::<TestDbHandle>(&[])
            .await
            .expect("install migration");
        let sql = AuthSession::<TestRole>::generate_unique_meta_key_pair_sql(
            "user_id",
            "server_tag",
        );
        saps::sqlx::raw_sql(saps::sqlx::AssertSqlSafe(sql))
            .execute(pool)
            .await
            .expect("install unique pair index");

        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "app"})),
        )
        .await
        .expect("first insert should succeed");

        let dup = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "app"})),
        )
        .await;
        let err = dup.expect_err("duplicate (user_id, server_tag) pair must fail");
        let db_err = match err {
            sqlx::Error::Database(db_err) => db_err,
            other => panic!("expected database error, got {:?}", other),
        };
        assert!(
            db_err.is_unique_violation(),
            "expected unique violation, got: {}",
            db_err
        );

        // Same user on a different server — pair is distinct, so allowed.
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "metrics"})),
        )
        .await
        .expect("user_id=1 on a different server_tag should succeed");

        // Two rows with only user_id and no server_tag — partial index excludes them.
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("user_id=1 without server_tag should succeed");
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("second user_id=1 without server_tag should still succeed");

        // Different user on a different server — fully distinct pair.
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer)
                .with_meta(serde_json::json!({"user_id": 2, "server_tag": "metrics"})),
        )
        .await
        .expect("user_id=2 on a different server_tag should succeed");

        // Multiple rows with only server_tag and no user_id — partial index excludes them.
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"server_tag": "app"})),
        )
        .await
        .expect("server_tag=app without user_id should succeed");
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer)
                .with_meta(serde_json::json!({"server_tag": "app"})),
        )
        .await
        .expect("second server_tag=app without user_id should still succeed");
    }

    #[saps::db_test]
    async fn test_unique_meta_key_pair_allows_same_user_different_server() {
        AuthSession::<TestRole>::run_migration::<TestDbHandle>(&[])
            .await
            .expect("install migration");
        let sql = AuthSession::<TestRole>::generate_unique_meta_key_pair_sql(
            "user_id",
            "server_tag",
        );
        saps::sqlx::raw_sql(saps::sqlx::AssertSqlSafe(sql))
            .execute(pool)
            .await
            .expect("install unique pair index");

        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "app"})),
        )
        .await
        .expect("first insert");
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "metrics"})),
        )
        .await
        .expect("same user on a different server should be allowed");
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer)
                .with_meta(serde_json::json!({"user_id": 2, "server_tag": "app"})),
        )
        .await
        .expect("different user on same server should be allowed");

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await
            .expect("get all");
        assert_eq!(all.len(), 3);
    }

    #[saps::db_test]
    async fn test_unique_meta_key_pair_skips_when_either_key_missing() {
        AuthSession::<TestRole>::run_migration::<TestDbHandle>(&[])
            .await
            .expect("install migration");
        let sql = AuthSession::<TestRole>::generate_unique_meta_key_pair_sql(
            "user_id",
            "server_tag",
        );
        saps::sqlx::raw_sql(saps::sqlx::AssertSqlSafe(sql))
            .execute(pool)
            .await
            .expect("install unique pair index");

        // No meta at all — excluded.
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(AuthSession::new(
            TestRole::Admin,
        ))
        .await
        .expect("insert without meta");
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(AuthSession::new(
            TestRole::Customer,
        ))
        .await
        .expect("second insert without meta");

        // Only one of the two keys present — excluded.
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("insert with only user_id");
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("duplicate user_id without server_tag should still be allowed");

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await
            .expect("get all");
        assert_eq!(all.len(), 4);
    }

    #[saps::db_test]
    async fn test_unique_meta_key_partial_index_skips_missing_key() {
        AuthSession::<TestRole>::run_migration::<TestDbHandle>(&["user_id"])
            .await
            .expect("install unique meta key index");

        // Two sessions with no meta at all — partial index excludes them.
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(AuthSession::new(
            TestRole::Admin,
        ))
        .await
        .expect("first insert without meta");
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(AuthSession::new(
            TestRole::Customer,
        ))
        .await
        .expect("second insert without meta");

        // A session with a different key in meta also skips the index.
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"team": "backend"})),
        )
        .await
        .expect("insert with unrelated meta key");

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await
            .expect("get all");
        assert_eq!(all.len(), 3);
    }

    #[saps::db_test]
    async fn test_upsert_meta_by_meta_inserts_new_key_and_preserves_others() {
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "team": "backend"})),
        )
        .await
        .expect("create");

        let count =
            AuthPostGresDescriptor::<TestDbHandle>::upsert_auth_sessions_meta_key_by_meta(
                "user_id",
                serde_json::json!(1),
                "level",
                serde_json::json!(3),
            )
            .await
            .expect("upsert by meta");
        assert_eq!(count, 1);

        let fetched = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session::<TestRole>(
            &created.id.to_string(),
        )
        .await
        .expect("get session")
        .expect("session should exist");
        assert_eq!(
            fetched.meta,
            Some(serde_json::json!({"user_id": 1, "team": "backend", "level": 3}))
        );
    }

    #[saps::db_test]
    async fn test_upsert_meta_by_meta_overwrites_existing_key() {
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer)
                .with_meta(serde_json::json!({"user_id": 1, "team": "backend"})),
        )
        .await
        .expect("create");

        let count =
            AuthPostGresDescriptor::<TestDbHandle>::upsert_auth_sessions_meta_key_by_meta(
                "user_id",
                serde_json::json!(1),
                "team",
                serde_json::json!("platform"),
            )
            .await
            .expect("upsert by meta");
        assert_eq!(count, 1);

        let fetched = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session::<TestRole>(
            &created.id.to_string(),
        )
        .await
        .expect("get session")
        .expect("session should exist");
        assert_eq!(
            fetched.meta,
            Some(serde_json::json!({"user_id": 1, "team": "platform"}))
        );
    }

    #[saps::db_test]
    async fn test_upsert_meta_by_meta_no_matches_returns_zero() {
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create");

        let count =
            AuthPostGresDescriptor::<TestDbHandle>::upsert_auth_sessions_meta_key_by_meta(
                "user_id",
                serde_json::json!(99),
                "level",
                serde_json::json!(3),
            )
            .await
            .expect("upsert by meta");
        assert_eq!(count, 0);

        let fetched = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session::<TestRole>(
            &created.id.to_string(),
        )
        .await
        .expect("get session")
        .expect("session should exist");
        assert_eq!(fetched.meta, Some(serde_json::json!({"user_id": 1})));
    }

    #[saps::db_test]
    async fn test_upsert_meta_by_meta_updates_every_match() {
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create a");
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create b");
        let untouched = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 2})),
        )
        .await
        .expect("create c");

        let count =
            AuthPostGresDescriptor::<TestDbHandle>::upsert_auth_sessions_meta_key_by_meta(
                "user_id",
                serde_json::json!(1),
                "flag",
                serde_json::json!(true),
            )
            .await
            .expect("upsert by meta");
        assert_eq!(count, 2);

        let matches =
            AuthPostGresDescriptor::<TestDbHandle>::get_auth_sessions_by_meta_key::<TestRole>(
                "flag",
                serde_json::json!(true),
            )
            .await
            .expect("get by meta key");
        assert_eq!(matches.len(), 2);

        let unchanged = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session::<TestRole>(
            &untouched.id.to_string(),
        )
        .await
        .expect("get session")
        .expect("session should exist");
        assert_eq!(unchanged.meta, Some(serde_json::json!({"user_id": 2})));
    }

    #[saps::db_test]
    async fn test_upsert_meta_by_meta_key_pair_updates_only_matching_pair() {
        let target = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "app"})),
        )
        .await
        .expect("create target");
        let other_server = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "metrics"})),
        )
        .await
        .expect("create other server");
        let other_user = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer)
                .with_meta(serde_json::json!({"user_id": 2, "server_tag": "app"})),
        )
        .await
        .expect("create other user");

        let count = AuthPostGresDescriptor::<TestDbHandle>
            ::upsert_auth_sessions_meta_key_by_meta_key_pair(
                "user_id",
                serde_json::json!(1),
                "server_tag",
                serde_json::json!("app"),
                "level",
                serde_json::json!(3),
            )
            .await
            .expect("upsert by meta key pair");
        assert_eq!(count, 1);

        let updated = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session::<TestRole>(
            &target.id.to_string(),
        )
        .await
        .expect("get target")
        .expect("target should exist");
        assert_eq!(
            updated.meta,
            Some(serde_json::json!({"user_id": 1, "server_tag": "app", "level": 3}))
        );

        // Other rows must be untouched.
        let other_server = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session::<TestRole>(
            &other_server.id.to_string(),
        )
        .await
        .expect("get other server")
        .expect("session should exist");
        assert_eq!(
            other_server.meta,
            Some(serde_json::json!({"user_id": 1, "server_tag": "metrics"}))
        );
        let other_user = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session::<TestRole>(
            &other_user.id.to_string(),
        )
        .await
        .expect("get other user")
        .expect("session should exist");
        assert_eq!(
            other_user.meta,
            Some(serde_json::json!({"user_id": 2, "server_tag": "app"}))
        );
    }

    #[saps::db_test]
    async fn test_upsert_meta_by_meta_key_pair_overwrites_existing_key() {
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(
                serde_json::json!({"user_id": 1, "server_tag": "app", "level": 1}),
            ),
        )
        .await
        .expect("create");

        let count = AuthPostGresDescriptor::<TestDbHandle>
            ::upsert_auth_sessions_meta_key_by_meta_key_pair(
                "user_id",
                serde_json::json!(1),
                "server_tag",
                serde_json::json!("app"),
                "level",
                serde_json::json!(5),
            )
            .await
            .expect("upsert by meta key pair");
        assert_eq!(count, 1);

        let fetched = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session::<TestRole>(
            &created.id.to_string(),
        )
        .await
        .expect("get")
        .expect("session should exist");
        assert_eq!(
            fetched.meta,
            Some(serde_json::json!({"user_id": 1, "server_tag": "app", "level": 5}))
        );
    }

    #[saps::db_test]
    async fn test_upsert_meta_by_meta_key_pair_upsert_key_is_one_of_match_keys() {
        let target = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "app"})),
        )
        .await
        .expect("create target");
        // Sibling row that must not be touched.
        let sibling = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "metrics"})),
        )
        .await
        .expect("create sibling");

        // Match by (user_id=1, server_tag="app") and then overwrite server_tag itself.
        let count = AuthPostGresDescriptor::<TestDbHandle>
            ::upsert_auth_sessions_meta_key_by_meta_key_pair(
                "user_id",
                serde_json::json!(1),
                "server_tag",
                serde_json::json!("app"),
                "server_tag",
                serde_json::json!("stage"),
            )
            .await
            .expect("upsert by meta key pair");
        assert_eq!(count, 1);

        let updated = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session::<TestRole>(
            &target.id.to_string(),
        )
        .await
        .expect("get target")
        .expect("target should exist");
        assert_eq!(
            updated.meta,
            Some(serde_json::json!({"user_id": 1, "server_tag": "stage"}))
        );

        // Sibling untouched — confirms WHERE matched on pre-update state and
        // didn't cascade to the row that now shares its old server_tag value.
        let sibling = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session::<TestRole>(
            &sibling.id.to_string(),
        )
        .await
        .expect("get sibling")
        .expect("session should exist");
        assert_eq!(
            sibling.meta,
            Some(serde_json::json!({"user_id": 1, "server_tag": "metrics"}))
        );
    }

    #[saps::db_test]
    async fn test_upsert_meta_by_meta_key_pair_no_matches_returns_zero() {
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin)
                .with_meta(serde_json::json!({"user_id": 1, "server_tag": "app"})),
        )
        .await
        .expect("create");

        let count = AuthPostGresDescriptor::<TestDbHandle>
            ::upsert_auth_sessions_meta_key_by_meta_key_pair(
                "user_id",
                serde_json::json!(1),
                "server_tag",
                serde_json::json!("metrics"),
                "level",
                serde_json::json!(3),
            )
            .await
            .expect("upsert by meta key pair");
        assert_eq!(count, 0);

        let unchanged = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session::<TestRole>(
            &created.id.to_string(),
        )
        .await
        .expect("get")
        .expect("session should exist");
        assert_eq!(
            unchanged.meta,
            Some(serde_json::json!({"user_id": 1, "server_tag": "app"}))
        );
    }

    #[test]
    fn test_meta_get_returns_value_when_key_exists() {
        let session = AuthSession::new(TestRole::Admin)
            .with_meta(serde_json::json!({"user_id": 7, "team": "backend"}));
        assert_eq!(session.meta_get_owned("user_id"), Some(serde_json::json!(7)));
        assert_eq!(session.meta_get_owned("team"), Some(serde_json::json!("backend")));
    }

    #[test]
    fn test_meta_get_returns_none_when_key_missing() {
        let session =
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 7}));
        assert_eq!(session.meta_get_owned("missing"), None);
    }

    #[test]
    fn test_meta_get_returns_none_when_meta_is_none() {
        let session = AuthSession::<TestRole>::new(TestRole::Admin);
        assert!(session.meta.is_none());
        assert_eq!(session.meta_get_owned("user_id"), None);
    }

    #[test]
    fn test_meta_get_strict_returns_value_when_key_exists() {
        let session =
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 7}));
        let value = session.meta_get_strict_owned("user_id").expect("present");
        assert_eq!(value, serde_json::json!(7));
    }

    #[test]
    fn test_meta_get_strict_not_found_when_key_missing() {
        let session =
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 7}));
        let err = session.meta_get_strict_owned("missing").expect_err("missing");
        assert_eq!(err.status, SapsErrorStatus::NotFound);
    }

    #[test]
    fn test_meta_get_strict_not_found_when_meta_is_none() {
        let session = AuthSession::<TestRole>::new(TestRole::Admin);
        let err = session.meta_get_strict_owned("user_id").expect_err("missing");
        assert_eq!(err.status, SapsErrorStatus::NotFound);
    }

    #[test]
    fn test_meta_get_typed_returns_some_typed_value() {
        let session =
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 7}));
        let value: Option<i32> = session.meta_get_typed_owned("user_id").expect("decoded");
        assert_eq!(value, Some(7));
    }

    #[test]
    fn test_meta_get_typed_returns_none_when_key_missing() {
        let session =
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 7}));
        let value: Option<i32> = session.meta_get_typed_owned("missing").expect("ok");
        assert_eq!(value, None);
    }

    #[test]
    fn test_meta_get_typed_bad_request_on_type_mismatch() {
        let session = AuthSession::new(TestRole::Admin)
            .with_meta(serde_json::json!({"user_id": "not-a-number"}));
        let result: Result<Option<i32>, _> = session.meta_get_typed_owned("user_id");
        let err = result.expect_err("should fail to decode");
        assert_eq!(err.status, SapsErrorStatus::BadRequest);
    }

    #[test]
    fn test_meta_get_typed_strict_returns_value() {
        let session =
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 7}));
        let value: i32 = session.meta_get_typed_strict_owned("user_id").expect("decoded");
        assert_eq!(value, 7);
    }

    #[test]
    fn test_meta_get_typed_strict_not_found_when_key_missing() {
        let session =
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 7}));
        let err = session
            .meta_get_typed_strict_owned::<i32>("missing")
            .expect_err("missing");
        assert_eq!(err.status, SapsErrorStatus::NotFound);
    }

    #[test]
    fn test_meta_get_typed_strict_bad_request_on_type_mismatch() {
        let session = AuthSession::new(TestRole::Admin)
            .with_meta(serde_json::json!({"user_id": "not-a-number"}));
        let err = session
            .meta_get_typed_strict_owned::<i32>("user_id")
            .expect_err("bad value");
        assert_eq!(err.status, SapsErrorStatus::BadRequest);
    }

    #[test]
    fn test_meta_get_returns_ref_to_value() {
        let session = AuthSession::new(TestRole::Admin)
            .with_meta(serde_json::json!({"user_id": 7, "team": "backend"}));
        let v: &serde_json::Value = session.meta_get("user_id").expect("present");
        assert_eq!(*v, serde_json::json!(7));
        let team: &serde_json::Value = session.meta_get("team").expect("present");
        assert_eq!(team.as_str(), Some("backend"));
    }

    #[test]
    fn test_meta_get_ref_returns_none_when_missing() {
        let session =
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 7}));
        assert!(session.meta_get("missing").is_none());
        let empty = AuthSession::<TestRole>::new(TestRole::Admin);
        assert!(empty.meta_get("user_id").is_none());
    }

    #[test]
    fn test_meta_get_strict_returns_ref_to_value() {
        let session =
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 7}));
        let v: &serde_json::Value = session.meta_get_strict("user_id").expect("present");
        assert_eq!(*v, serde_json::json!(7));
    }

    #[test]
    fn test_meta_get_strict_ref_not_found_when_missing() {
        let session = AuthSession::<TestRole>::new(TestRole::Admin);
        let err = session.meta_get_strict("user_id").expect_err("missing");
        assert_eq!(err.status, SapsErrorStatus::NotFound);
    }

    #[test]
    fn test_meta_get_typed_borrows_str() {
        let session = AuthSession::new(TestRole::Admin)
            .with_meta(serde_json::json!({"team": "backend"}));
        // &str borrows directly from the underlying Value's storage.
        let team: Option<&str> = session.meta_get_typed("team").expect("decoded");
        assert_eq!(team, Some("backend"));
    }

    #[test]
    fn test_meta_get_typed_owned_types_still_work() {
        let session =
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 7}));
        let v: Option<i32> = session.meta_get_typed("user_id").expect("decoded");
        assert_eq!(v, Some(7));
    }

    #[test]
    fn test_meta_get_typed_strict_borrows_str() {
        let session = AuthSession::new(TestRole::Admin)
            .with_meta(serde_json::json!({"team": "backend"}));
        let team: &str = session.meta_get_typed_strict("team").expect("decoded");
        assert_eq!(team, "backend");
    }

    #[test]
    fn test_meta_get_typed_strict_not_found_when_missing() {
        let session = AuthSession::<TestRole>::new(TestRole::Admin);
        let err = session
            .meta_get_typed_strict::<i32>("user_id")
            .expect_err("missing");
        assert_eq!(err.status, SapsErrorStatus::NotFound);
    }

    #[saps::db_test]
    async fn test_get_auth_session_strict_returns_session() {
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create");

        let fetched =
            AuthPostGresDescriptor::<TestDbHandle>::get_auth_session_strict::<TestRole>(
                &created.id.to_string(),
            )
            .await
            .expect("strict get should find session");
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.role, TestRole::Admin);
        assert_eq!(fetched.meta, Some(serde_json::json!({"user_id": 1})));
    }

    #[saps::db_test]
    async fn test_get_auth_session_strict_row_not_found() {
        let fake_id = uuid::Uuid::new_v4().to_string();
        let err = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session_strict::<TestRole>(
            &fake_id,
        )
        .await
        .expect_err("missing session should error");
        assert!(
            matches!(err, sqlx::Error::RowNotFound),
            "expected RowNotFound, got {:?}",
            err
        );
    }

    #[saps::db_test]
    async fn test_get_auth_session_strict_invalid_uuid_errors() {
        let err = AuthPostGresDescriptor::<TestDbHandle>::get_auth_session_strict::<TestRole>(
            "not-a-uuid",
        )
        .await
        .expect_err("invalid uuid should error");
        assert!(
            matches!(err, sqlx::Error::Protocol(_)),
            "expected Protocol error, got {:?}",
            err
        );
    }

    #[saps::db_test]
    async fn test_get_auth_session_by_meta_key_strict_returns_session() {
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Customer).with_meta(serde_json::json!({"user_id": 7})),
        )
        .await
        .expect("create");

        let fetched =
            AuthPostGresDescriptor::<TestDbHandle>::get_auth_session_by_meta_key_strict::<TestRole>(
                "user_id",
                serde_json::json!(7),
            )
            .await
            .expect("strict get should find session");
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.role, TestRole::Customer);
    }

    #[saps::db_test]
    async fn test_get_auth_session_by_meta_key_strict_row_not_found() {
        let _ = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(
            AuthSession::new(TestRole::Admin).with_meta(serde_json::json!({"user_id": 1})),
        )
        .await
        .expect("create");

        let err =
            AuthPostGresDescriptor::<TestDbHandle>::get_auth_session_by_meta_key_strict::<TestRole>(
                "user_id",
                serde_json::json!(99),
            )
            .await
            .expect_err("no matching session should error");
        assert!(
            matches!(err, sqlx::Error::RowNotFound),
            "expected RowNotFound, got {:?}",
            err
        );
    }
}
