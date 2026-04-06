use crate::define_dal_transactions;
use crate::auth::token::checks::UserRole;
use super::model::AuthSession;

define_dal_transactions!(
    CreateAuthSession => create_auth_session[U: UserRole](session: AuthSession<U>) -> AuthSession<U>,
    PingAuthSession => ping_auth_session[U: UserRole](minutes: i32, session_id: &str) -> Option<AuthSession<U>>,
    DeleteAuthSession => delete_auth_session(session_id: uuid::Uuid) -> bool,
    GetAllAuthSessions => get_all_auth_sessions[U: UserRole]() -> Vec<AuthSession<U>>
);
