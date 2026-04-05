//! Creates a connection pool for the postgres database.
extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{Ident, LitStr, Token, parse::Parse, parse::ParseStream, parse_macro_input};

/// The input args into the database.
struct DbPoolArgs {
    /// The name of the connection pool to be referenced throughout the program
    pool_ident: Ident,
    /// The string URL env variable for the DB connection
    url_env: LitStr,
    /// The string env variable for the maximum number of connections
    max_conn_env: LitStr,
}

impl Parse for DbPoolArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let pool_ident: Ident = input.parse()?;
        input.parse::<Token![,]>()?;
        let url_env: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let max_conn_env: LitStr = input.parse()?;
        Ok(DbPoolArgs {
            pool_ident,
            url_env,
            max_conn_env,
        })
    }
}

#[proc_macro]
pub fn define_pg_pool(input: TokenStream) -> TokenStream {
    let DbPoolArgs {
        pool_ident,
        url_env,
        max_conn_env,
    } = parse_macro_input!(input as DbPoolArgs);

    quote! {
        pub static #pool_ident: std::sync::LazyLock<saps::sqlx::postgres::PgPool> = std::sync::LazyLock::new(|| {
            let connection_string = std::env::var(#url_env).expect("Get env variable for connection string");

            let max_connections = match std::env::var(#max_conn_env) {
                Ok(val) => val,
                Err(_) => "5".to_string()
            }.trim().parse::<u32>().map_err(|_e| {
                format!("Could not parse {} as max connections", #max_conn_env)
            }).expect("part connections");

            let pool = saps::sqlx::postgres::PgPoolOptions::new()
                .max_connections(max_connections);

            pool.connect_lazy(&connection_string)
                .expect("Failed to create pool")
        });
    }
    .into()
}
