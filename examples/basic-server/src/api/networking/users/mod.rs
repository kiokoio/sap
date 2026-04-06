pub mod create;
pub mod delete;
pub mod get;
pub mod login;
pub mod logout;

use saps::axum::{Router, routing::{get, post, delete as delete_method}};
use saps::config::GetConfigVariable;
use saps::dal::connections::{
    LivePostGresPool, SqlxPostGresDescriptor, AuthPostGresDescriptor,
};
use crate::roles::{Role, NoRoleCheck};

/// Attaches all user-related views to the router.
///
/// # Type Parameters
/// * `C` - A type that implements `GetConfigVariable` (e.g. `EnvConfig` or a test config)
pub fn users_factory<C>(app: Router) -> Router
where
    C: GetConfigVariable + Send + Sync + 'static,
{
    app.route(
        "/api/v1/users",
        post(
            create::create_user_handler::<SqlxPostGresDescriptor<LivePostGresPool>>,
        ),
    )
    .route(
        "/api/v1/users/me",
        get(
            get::get_user_handler::<
                SqlxPostGresDescriptor<LivePostGresPool>,
                C,
                NoRoleCheck,
                Role,
                LivePostGresPool,
            >,
        ),
    )
    .route(
        "/api/v1/auth/logout",
        post(
            logout::logout_handler::<
                AuthPostGresDescriptor<LivePostGresPool>,
                C,
                NoRoleCheck,
                Role,
                LivePostGresPool,
            >,
        ),
    )
    .route(
        "/api/v1/users",
        delete_method(
            delete::delete_user_handler::<
                SqlxPostGresDescriptor<LivePostGresPool>,
                C,
                NoRoleCheck,
                Role,
                LivePostGresPool,
            >,
        ),
    )
}
