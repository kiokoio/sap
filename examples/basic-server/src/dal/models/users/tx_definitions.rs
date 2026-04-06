use saps::define_dal_transactions;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct User {
    pub id: uuid::Uuid,
    pub username: String,
    pub email: String,
    pub password_hash: String,
}

define_dal_transactions!(
    CreateUser => create_user(username: String, email: String, password_hash: String) -> User,
    GetUserByEmail => get_user_by_email(email: String) -> Option<User>,
    GetUserById => get_user_by_id(user_id: uuid::Uuid) -> Option<User>,
    DeleteUser => delete_user(user_id: uuid::Uuid) -> bool
);
