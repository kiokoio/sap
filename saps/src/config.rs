//! Defines extracting config variables.
use crate::errors::saps::SapsError;
use std::env;

/// Defines the trait for getting config variables
pub trait GetConfigVariable {
    /// Gets the config variable
    ///
    /// # Arguments
    /// * `variable` - The name of the config variable to get
    ///
    /// # Returns
    /// * `Result<String, SapsError>` - The result of getting the config variable
    fn get_config_variable(variable: String) -> Result<String, SapsError>;
}

/// Defines the struct for getting config variables from the environment
pub struct EnvConfig;

impl GetConfigVariable for EnvConfig {
    /// Gets the config variable from the environment
    ///
    /// # Arguments
    /// * `variable` - The name of the config variable to get
    ///
    /// # Returns
    /// * `Result<String, SapsError>` - The result of getting the config variable
    fn get_config_variable(variable: String) -> Result<String, SapsError> {
        match env::var(&variable) {
            Ok(val) => Ok(val),
            Err(_) => {
                Err(SapsError::unknown(format!("{} not found in environment", variable)))
            },
        }
    }
}


#[macro_export]
macro_rules! define_static_config {
    ($handle:ident, $( $key:expr => $value:expr ),*) => {
        #[derive(Clone)]
        pub struct $handle;
        impl kernel::config::GetConfigVariable for $handle {
            fn get_config_variable(variable: String) -> Result<String, saps::errors::saps::SapsError> {
                match variable.as_str() {
                    $(
                        $key => Ok($value.to_string()),
                    )*
                    _ => Err(saps::errors::saps::SapsError::unknown(
                        format!("key: {} was not found", variable)
                    ))
                }
            }
        }
    };
    (DEFAULT) => {
        define_static_config!(
            DefaultConfig,
            "FRONTEND_DOMAIN" => "test_domain",
            "MAILCHIMP_API_KEY" => "mock_mailchimp_api",
            "PRODUCTION" => "true",
            "RATE_LIMIT_PERIOD_MINUTES" => "60",
            "RATE_LIMIT" => "5",
            "SECRET_KEY" => "secret",
            "SERVER_TAG" => "test_server"
        );
    };
}

/// Generates a config struct backed by `OnceLock` static variables loaded from the environment.
///
/// Unlike [`define_static_config!`] which maps keys to hardcoded values, this macro takes a
/// list of environment variable names and creates:
///
/// 1. One `OnceLock<String>` static per key
/// 2. A struct that implements [`GetConfigVariable`]
/// 3. An `init()` function on the struct that reads each key from the environment and sets
///    the corresponding `OnceLock`
///
/// # Syntax
///
/// ```text
/// define_env_config!(MyConfig, "SECRET_KEY", "DATABASE_URL", "TOKEN_EXPIRE_MINS");
/// ```
///
/// # Usage
///
/// ```ignore
/// define_env_config!(AppConfig, "SECRET_KEY", "DATABASE_URL");
///
/// // Call once at startup to load from the environment
/// AppConfig::init().expect("failed to load config");
///
/// // Then use via the trait
/// let secret = AppConfig::get_config_variable("SECRET_KEY".into()).unwrap();
/// ```
#[macro_export]
macro_rules! define_env_config {
    ($handle:ident, $( $key:expr ),* $(,)?) => {
        paste::paste! {
            $(
                static [< __CONFIG_ $handle:upper _ $key:upper >]: std::sync::OnceLock<String> = std::sync::OnceLock::new();
            )*

            #[derive(Clone)]
            pub struct $handle;

            impl $handle {
                /// Reads each config key from the environment and stores it in a `OnceLock`.
                /// Call this once at startup. Returns an error if any key is missing.
                pub fn init() -> Result<(), saps::errors::saps::SapsError> {
                    $(
                        let val = std::env::var($key).map_err(|_| {
                            saps::errors::saps::SapsError::unknown(
                                format!("{} not found in environment", $key)
                            )
                        })?;
                        [< __CONFIG_ $handle:upper _ $key:upper >].set(val).ok();
                    )*
                    Ok(())
                }
            }

            impl saps::config::GetConfigVariable for $handle {
                fn get_config_variable(variable: String) -> Result<String, saps::errors::saps::SapsError> {
                    match variable.as_str() {
                        $(
                            $key => [< __CONFIG_ $handle:upper _ $key:upper >]
                                .get()
                                .cloned()
                                .ok_or_else(|| saps::errors::saps::SapsError::unknown(
                                    format!("{} not initialised — call {}::init() first", $key, stringify!($handle))
                                )),
                        )*
                        _ => Err(saps::errors::saps::SapsError::unknown(
                            format!("key: {} was not found in {}", variable, stringify!($handle))
                        ))
                    }
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    
    use super::*;

    define_env_config!(TestConfig, "TEST_SECRET_KEY", "TEST_DB_URL");

    #[test]
    fn test_init_and_get_config_variable() {
        // Set env vars before init
        unsafe {
            std::env::set_var("TEST_SECRET_KEY", "my_secret");
            std::env::set_var("TEST_DB_URL", "postgres://localhost/test");
        }

        TestConfig::init().expect("init should succeed");

        let secret = TestConfig::get_config_variable("TEST_SECRET_KEY".into()).unwrap();
        assert_eq!(secret, "my_secret");

        let db_url = TestConfig::get_config_variable("TEST_DB_URL".into()).unwrap();
        assert_eq!(db_url, "postgres://localhost/test");
    }

    #[test]
    fn test_get_unknown_key_returns_error() {
        let result = TestConfig::get_config_variable("NONEXISTENT_KEY".into());
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("was not found in TestConfig"));
    }

    define_env_config!(UninitConfig, "UNINIT_VAR_XYZ");

    #[test]
    fn test_get_before_init_returns_error() {
        // Don't call init — OnceLock is empty
        let result = UninitConfig::get_config_variable("UNINIT_VAR_XYZ".into());
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("not initialised"));
    }

    define_env_config!(MissingEnvConfig, "THIS_VAR_DOES_NOT_EXIST_12345");

    #[test]
    fn test_init_fails_when_env_var_missing() {
        let result = MissingEnvConfig::init();
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("not found in environment"));
    }
}
