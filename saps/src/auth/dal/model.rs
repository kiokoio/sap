use uuid::Uuid;
use crate::auth::token::checks::UserRole;
use crate::errors::saps::SapsError;
use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::{Row, postgres::PgRow};


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
        let role_str: String = row.try_get("role")
            .map_err(|e| SapsError::unknown(e.to_string()))?;
        let role = U::try_from(role_str)?;
        Ok(Self {
            id: row.try_get("id").map_err(|e| SapsError::unknown(e.to_string()))?,
            role,
            date_created: row.try_get("date_created").map_err(|e| SapsError::unknown(e.to_string()))?,
            last_interacted: row.try_get("last_interacted").map_err(|e| SapsError::unknown(e.to_string()))?,
            meta: row.try_get("meta").map_err(|e| SapsError::unknown(e.to_string()))?,
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

    /// Returns a SQL script that creates the `saps` schema (if it doesn't exist)
    /// and the `saps.auth_sessions` table (if it doesn't exist) matching this struct's fields.
    pub fn generate_migration_sql() -> &'static str {
        r#"
CREATE SCHEMA IF NOT EXISTS saps;

CREATE TABLE IF NOT EXISTS saps.auth_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    role VARCHAR(255) NOT NULL,
    date_created TIMESTAMP NOT NULL DEFAULT NOW(),
    last_interacted TIMESTAMP NOT NULL DEFAULT NOW(),
    meta JSONB
);

CREATE INDEX IF NOT EXISTS idx_saps_auth_sessions_last_interacted
    ON saps.auth_sessions (last_interacted);

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
    WHERE id = p_session_id
      AND last_interacted < NOW() - (p_minutes || ' minutes')::INTERVAL;

    GET DIAGNOSTICS rows_affected = ROW_COUNT;

    IF rows_affected > 0 THEN
        RETURN NULL;
    END IF;

    UPDATE saps.auth_sessions
    SET last_interacted = NOW()
    WHERE id = p_session_id
    RETURNING * INTO session_record;

    GET DIAGNOSTICS rows_affected = ROW_COUNT;

    IF rows_affected = 0 THEN
        RETURN NULL;
    END IF;

    RETURN session_record;
END;
$$;
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::dal::tx_definitions::{CreateAuthSession, DeleteAuthSession, GetAllAuthSessions, PingAuthSession};
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
            .await.expect("failed to get all sessions");
        assert_eq!(all.len(), 0);

        let session = AuthSession::new(TestRole::Admin);
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create auth session");
        assert_eq!(created.role, TestRole::Admin);
        assert!(created.meta.is_none());

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await.expect("failed to get all sessions");
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

        let pinged = AuthPostGresDescriptor::<TestDbHandle>::ping_auth_session::<TestRole>(30, &created.id.to_string())
            .await
            .expect("failed to ping session");
        assert!(pinged.is_some());
        let pinged = pinged.unwrap();
        assert_eq!(pinged.role, TestRole::Admin);
    }

    #[saps::db_test]
    async fn test_ping_nonexistent_session_returns_none() {
        let fake_id = uuid::Uuid::new_v4().to_string();
        let pinged = AuthPostGresDescriptor::<TestDbHandle>::ping_auth_session::<TestRole>(30, &fake_id)
            .await
            .expect("failed to ping session");
        assert!(pinged.is_none());
    }

    #[saps::db_test]
    async fn test_ping_expired_session_returns_none() {
        let session = AuthSession::new(TestRole::Customer)
            .with_meta(serde_json::json!({"user_id": 4}));
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await.expect("failed to get all sessions");
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
        let pinged = AuthPostGresDescriptor::<TestDbHandle>::ping_auth_session::<TestRole>(30, &created.id.to_string())
            .await
            .expect("failed to ping session");
        assert!(pinged.is_none());

        // Expired session should have been deleted by ping
        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await.expect("failed to get all sessions");
        assert_eq!(all.len(), 0);
    }

    #[saps::db_test]
    async fn test_delete_auth_session() {
        let session = AuthSession::new(TestRole::Admin)
            .with_meta(serde_json::json!({"user_id": 5}));
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await.expect("failed to get all sessions");
        assert_eq!(all.len(), 1);

        let deleted = AuthPostGresDescriptor::<TestDbHandle>::delete_auth_session(created.id)
            .await
            .expect("failed to delete session");
        assert!(deleted);

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await.expect("failed to get all sessions");
        assert_eq!(all.len(), 0);
    }

    #[saps::db_test]
    async fn test_delete_nonexistent_session_returns_false() {
        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await.expect("failed to get all sessions");
        assert_eq!(all.len(), 0);

        let fake_id = uuid::Uuid::new_v4();
        let deleted = AuthPostGresDescriptor::<TestDbHandle>::delete_auth_session(fake_id)
            .await
            .expect("failed to delete session");
        assert!(!deleted);

        let all = AuthPostGresDescriptor::<TestDbHandle>::get_all_auth_sessions::<TestRole>()
            .await.expect("failed to get all sessions");
        assert_eq!(all.len(), 0);
    }

    #[saps::db_test]
    async fn test_create_and_ping_preserves_meta() {
        let meta = serde_json::json!({"user_id": 6, "team": "backend"});
        let session = AuthSession::new(TestRole::Admin).with_meta(meta.clone());
        let created = AuthPostGresDescriptor::<TestDbHandle>::create_auth_session(session)
            .await
            .expect("failed to create session");

        let pinged = AuthPostGresDescriptor::<TestDbHandle>::ping_auth_session::<TestRole>(30, &created.id.to_string())
            .await
            .expect("failed to ping session");
        let pinged = pinged.expect("session should exist");
        assert_eq!(pinged.meta, Some(meta));
    }
}
