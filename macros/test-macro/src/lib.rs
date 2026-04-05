//! We can use the macro with the following:
//! ```ignore
//! #[db_test]
//! async fn test_create_username_conflict() {
//!     let all_users = SqlxPostGresDescriptor::get_all_user_profiles(pool)
//!         .await
//!         .expect("get all users");
//!
//!     assert_eq!(0, all_users.len());
//!
//!     let new_user = gen_new_user_schema();
//!
//!     // TestDbHandle is provided by the test macro and yields the SQLX_POSTGRES_POOL
//!     let outcome =
//!         create_customer::<SqlxPostGresDescriptor, TestDbHandle, TestConfig>(Json(new_user))
//!             .await;
//!     assert_eq!(201, outcome.into_response().status());
//!     let mut new_user = gen_new_user_schema();
//!     new_user.email = "another_email".into();
//!     insert_duplicate::<TestDbHandle>(new_user, "username already exists").await;
//! }
//! ```
extern crate proc_macro;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{ItemFn, LitStr, parse_macro_input};
use uuid::Uuid;

#[proc_macro_attribute]
pub fn db_test(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);

    // Get the function name
    let func_name = &input_fn.sig.ident;
    let stmts = &input_fn.block.stmts; // Vec<Stmt>

    let db_tag = Uuid::new_v4().simple().to_string(); // "c0a801e6be7d470691ed7d087b4e1bfd"
    let env_name = format!("DATABASE_URL_{}", db_tag); // "DATABASE_URL_c0a801e6be7d…"
    let env_lit = LitStr::new(&env_name, Span::call_site()); // `syn::LitStr` → "DATABASE_URL_c0…"

    let db_url = format!("postgres://username:password@localhost:5434/{db_tag}");
    let db_name = LitStr::new(&db_tag, Span::call_site());
    let db_url_lit = LitStr::new(&db_url, Span::call_site());
    let pg_url_lit = LitStr::new(
        "postgres://username:password@localhost:5434/main_db",
        Span::call_site(),
    );

    let expanded = quote! {
        #[test]
        fn #func_name() {
            use saps::sqlx::Executor;
            use tokio::runtime::Builder;

            // set the environment variables for the DB
            unsafe {
                std::env::set_var(#env_lit, #db_url_lit);
                std::env::set_var("DB_MAX_CONNECTIONS", "1");
            }

            let rt = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("create Tokio runtime");

            // create the DB in the core DB
            rt.block_on(async {
                let db_name = #db_name;
                let mut admin = saps::sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(#pg_url_lit)
                .await.expect("make admin DB connection pool");
                admin.execute(format!(r#"CREATE DATABASE "{db_name}""#).as_str()).await.expect("execute create DB transaction");
                admin.close().await;
            });

            let test_result = rt.block_on(async {
                // define the DB pool which is std::sync::LazyLock<saps::sqlx::Pool<saps::sqlx::Postgres>> inner = Pool<saps::sqlx::Postgres>
                saps::define_pg_pool!(SQLX_POSTGRES_POOL, #env_lit, "DB_MAX_CONNECTIONS");

                #[derive(Clone, Debug)]
                struct TestDbHandle;

                impl saps::dal::connections::YieldPostGresPool for TestDbHandle {
                    fn yield_pool() -> &'static saps::sqlx::Pool<saps::sqlx::Postgres> {
                        &*SQLX_POSTGRES_POOL
                    }
                }

                // execute the testing code
                let handle = tokio::spawn(async {
                    let pool: &saps::sqlx::Pool<saps::sqlx::Postgres> = &*SQLX_POSTGRES_POOL;
                    saps::dal::migrations::run_migrations(pool).await;
                    #(#stmts)*
                });
                handle.await   // → Result<Result<(), _>, JoinError>
            });

            // drop the DB table
            rt.block_on(async {
                let db_name = #db_name;
                let mut admin = saps::sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect(#pg_url_lit)
                .await.expect("make admin DB connection pool");
                admin.execute(format!(r#"DROP DATABASE "{db_name}" WITH (FORCE)"#).as_str()).await.expect("execute drop DB transaction");
                admin.close().await;

                // // Always cleanup
                // SQLX_POSTGRES_POOL.close().await;
            });
            test_result.unwrap();
        }
    };
    TokenStream::from(expanded)
}
