// //! Defines the connection to the PostgreSQL database and the `SqlxPostGresDescriptor` for dependency injection.
// //!
// //! # Overview
// //! - Establishes a connection pool for a PostgreSQL database using the `sqlx` library.
// //! - Provides the `SqlxPostGresDescriptor` struct to serve as a handle for database-related operations.
// //! - Configures the connection pool using environment variables for flexibility and scalability.
// use sqlx::{PgPool, Pool, Postgres};
// use std::marker::PhantomData;
// use std::sync::LazyLock;

// /// A descriptor struct used for applying database traits and dependency injection.
// ///
// /// # Notes
// /// This struct is intended to be used as a handle for implementing database-related traits
// /// that define transactions or other interactions with the database.
// #[derive(Clone)]
// pub struct SqlxPostGresDescriptor<T: YieldPostGresPool> {
//     db_handle: PhantomData<T>,
// }

// /// A descriptor struct for yielding a live PostGres DB pool
// #[derive(Clone, Debug)]
// pub struct LivePostGresPool;

// db_pool_macro::define_pg_pool!(SQLX_POSTGRES_POOL, "DATABASE_URL", "DB_MAX_CONNECTIONS");

// pub trait YieldPostGresPool {
//     fn yield_pool() -> &'static Pool<Postgres>;
// }

// impl YieldPostGresPool for LivePostGresPool {
//     fn yield_pool() -> &'static Pool<Postgres> {
//         &SQLX_POSTGRES_POOL
//     }
// }

// /// Mock pool that will error if connection is utilised.
// ///
// /// # Notes
// /// Only use this handle to satisfy the trait bounds of an
// /// operation where the DB transaction is mocked
// #[derive(Clone)]
// pub struct MockDeadPostGresPool;

// pub static DEAD_SQLX_POSTGRES_POOL: LazyLock<PgPool> =
//     LazyLock::new(|| Pool::<Postgres>::connect_lazy("postgres://invalid").unwrap());

// impl YieldPostGresPool for MockDeadPostGresPool {
//     fn yield_pool() -> &'static Pool<Postgres> {
//         &DEAD_SQLX_POSTGRES_POOL
//     }
// }
