# Testing Macro

This macro is for enabling us to test a function against a postgres database with a DB name of a unique ID so we can have the test run live isolated DB transactions against the isolated postgres DB.

# Usage

The usage of the macro can be done with the following:

```rust
use test_macro::db_test;

#[db_test]
async fn test_one() {
    let one = 1;
    let two = 1;
    assert_eq!(one, two);
}
```

This expands to the following:

```rust
#[tokio::test]
async fn test_one() {
    // define the env vars for the "DATABASE_URL_<a generated uuid>"

    // create a DB connection and create the DB for the postgres docker container

    // create the Pool<sqlx::Postgres> wrapped in a std::sync::LazyLock

    // perform the test code
    let one = 1;
    let two = 1;
    assert_eq!(one, two);

    // create another DB connection to the main DB for the postgres docker container and drop the DB
}
```

## Basic Example

Below is how we have a basic test:

```rust
use dal::migrations::run_migrations;
use sqlx::Row; // Row gives us `.get::<T, _>()`
use test_macro::db_test;

/// End‑to‑end check that the db_test wrapper
/// - spins up its own database
/// - lets us create / insert / query
/// - cleans up afterwards
#[db_test]
async fn users_roundtrip() {
    // The wrapper your macro generates has already created a pool called
    // `SQLX_POSTGRES_POOL` (LazyLock<Pool<Postgres>>) and pointed it at
    // a fresh DB whose name is the UUID baked into this test.
    //
    // Grab a reference to it:
    let pool = &*SQLX_POSTGRES_POOL;
    run_migrations(pool).await;

    // 2 ▸ data --------------------------------------------------------------
    sqlx::query("INSERT INTO users (username) VALUES ($1)")
        .bind("alice")
        .execute(pool)
        .await
        .unwrap();

    sqlx::query("INSERT INTO users (username) VALUES ($1)")
        .bind("bob")
        .execute(pool)
        .await
        .unwrap();

    // 3 ▸ query & assertions ------------------------------------------------
    let rows = sqlx::query("SELECT username FROM users ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<String, _>("username"), "alice");
    assert_eq!(rows[1].get::<String, _>("username"), "bob");
}
```

# Things complete

The points below need to be clarified to finalise this macro:

- Interface for a DB transaction. We need to pass in the pool to the transaction
- Attributes to pass in such as the port, DB name, and IP address of the DB
- Attribute to keep the DB after the test for DB debugging
