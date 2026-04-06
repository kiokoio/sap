//! Role-based access control (RBAC) checks for token-authenticated requests.
//!
//! This module provides a trait-based system for validating whether a user's role has
//! sufficient permissions to perform a given action. You define your roles as enum variants
//! inside the [`construct_checks!`] macro, and it generates:
//!
//! 1. The role enum itself (with `Clone`, `Debug` derived)
//! 2. `ToString` impl — converts each variant to its lowercased name (e.g. `SuperAdmin` → `"superadmin"`)
//! 3. `TryFrom<String>` impl — parses a string (case-insensitive) back into a variant
//! 4. `UserRole` impl — marks the enum as a valid role type for the check system
//! 5. Check structs — unit structs that implement [`CheckUserRole`], each gating access to
//!    one or more role variants
//!
//! # Example
//!
//! ```ignore
//! use saps::auth::token::checks::{UserRole, CheckUserRole};
//! use saps::errors::saps::SapsError;
//!
//! // This single macro call generates:
//! //   - `enum MyRole { SuperAdmin, Admin, Customer }`
//! //   - `ToString`, `TryFrom<String>`, and `UserRole` impls for `MyRole`
//! //   - `SuperAdminOnly`, `AdminOrAbove`, `AnyRole` check structs
//! saps::auth::token::checks::construct_checks!(
//!     enum MyRole {
//!         SuperAdmin,
//!         Admin,
//!         Customer,
//!     }
//!
//!     SuperAdminOnly => SuperAdmin,
//!     AdminOrAbove => SuperAdmin | Admin,
//!     AnyRole => SuperAdmin | Admin | Customer,
//! );
//!
//! // AdminOrAbove allows "superadmin" and "admin", but rejects "customer"
//! let admin = MyRole::Admin;
//! assert!(AdminOrAbove::check_user_role(&admin).is_ok());
//!
//! let customer = MyRole::Customer;
//! assert!(AdminOrAbove::check_user_role(&customer).is_err());
//!
//! // String round-trip works automatically
//! let role = MyRole::try_from("SUPERADMIN".to_string()).unwrap();
//! assert_eq!(role.to_string(), "superadmin");
//! ```
//!
//! # Pre-built check structs
//!
//! The default invocation at the bottom of this module generates these check structs:
//!
//! | Struct                     | Allowed variants                       |
//! |----------------------------|----------------------------------------|
//! | `SuperAdminRoleCheck`      | `SuperAdmin`                           |
//! | `AdminRoleCheck`           | `SuperAdmin`, `Admin`                  |
//! | `CustomerRoleCheck`        | `SuperAdmin`, `Admin`, `Customer`      |
//! | `NoRoleCheck`              | `SuperAdmin`, `Admin`, `Customer`      |
//! | `ExactSuperAdminRoleCheck` | `SuperAdmin` only                      |
//! | `ExactAdminRoleCheck`      | `Admin` only                           |
//! | `ExactCustomerRoleCheck`   | `Customer` only                        |
//!
//! The "hierarchy" checks (`SuperAdminRoleCheck`, `AdminRoleCheck`, `CustomerRoleCheck`) allow
//! higher-privilege roles to pass — e.g. a superadmin can always pass an `AdminRoleCheck`.
//! The "exact" checks (`ExactSuperAdminRoleCheck`, etc.) require the role to match exactly.

use serde::{Serialize, de::DeserializeOwned};

use crate::errors::saps::SapsError;

/// A trait for structs that gate access based on a user's role.
///
/// Each implementor defines which roles are permitted. Call `check_user_role` with the
/// user's role to get `Ok(())` if access is granted, or a `SapsError` with
/// `Unauthorized` status if denied.
///
/// You typically don't implement this trait manually — instead, use the [`construct_checks!`]
/// macro to generate check structs declaratively.
pub trait CheckUserRole {
    fn check_user_role<U: UserRole>(role: &U) -> Result<(), SapsError>;
}

/// A trait that must be implemented by your application's role type.
///
/// This trait has two supertraits:
/// - `ToString` — converts the role to its string representation (e.g. `"admin"`).
///   The string is lowercased during comparison, so `"Admin"` and `"admin"` are equivalent.
/// - `TryFrom<String, Error = SapsError>` — parses a role from a string (e.g. from a JWT claim).
///   Return a `SapsError` if the string doesn't match any known role.
///
/// The trait itself has no methods — it serves as a marker that ties together the
/// string conversion capabilities needed by the check system.
///
/// When using [`construct_checks!`] with an enum definition, all three impls (`ToString`,
/// `TryFrom<String>`, and `UserRole`) are generated automatically.
pub trait UserRole: ToString + TryFrom<String, Error = SapsError> + Send + Unpin + Serialize + DeserializeOwned {}

/// Generates a role enum, its string conversions, and role-check structs.
///
/// # Syntax
///
/// ```text
/// construct_checks!(
///     enum MyRole {
///         VariantA,
///         VariantB,
///         VariantC,
///     }
///
///     CheckStructA => VariantA,
///     CheckStructB => VariantA | VariantB,
///     CheckStructC => VariantA | VariantB | VariantC,
/// );
/// ```
///
/// # What gets generated
///
/// 1. **The enum** — `#[derive(Clone, Debug)] pub enum MyRole { VariantA, VariantB, VariantC }`
/// 2. **`ToString`** — each variant is converted to its lowercased name
///    (e.g. `VariantA` → `"varianta"`)
/// 3. **`TryFrom<String>`** — case-insensitive parsing from a string back to a variant.
///    Returns `SapsError::bad_request` for unknown strings.
/// 4. **`UserRole`** — marker trait impl so the enum can be used with `CheckUserRole`.
/// 5. **Check structs** — for each `CheckStructX => Variant | Variant` entry, a unit struct
///    is created that implements `CheckUserRole`. The check converts the role to a lowercase
///    string and compares it against the lowercased names of the allowed variants.
///
/// # Fragment types
///
/// - `$enum_name:ident` — the name of the generated role enum.
/// - `$variant:ident` — enum variant names (used both in the enum definition and in check rules).
/// - `$struct:ident` — the name of each generated check struct.
/// - `$role:ident` — one or more variant names separated by `|` in check rules. We use `ident`
///   so the `|` separator is valid in macro grammar (unlike `expr` which disallows `|` followers).
/// - Trailing commas are optional in both the enum body and the check rules.
#[macro_export]
macro_rules! construct_checks {
    (
        enum $enum_name:ident {
            $( $variant:ident ),* $(,)?
        }

        $( $struct:ident => $( $role:ident )|+ ),* $(,)?
    ) => {
        // --- 1. Generate the role enum ---
        #[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
        pub enum $enum_name {
            $( $variant ),*
        }

        // --- 2. Generate Display (each variant → lowercased name) ---
        impl std::fmt::Display for $enum_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $( $enum_name::$variant => write!(f, "{}", stringify!($variant).to_lowercase()) ),*
                }
            }
        }

        // --- 3. Generate TryFrom<String> (case-insensitive parsing) ---
        impl TryFrom<String> for $enum_name {
            type Error = saps::errors::saps::SapsError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                match value.to_lowercase().as_str() {
                    $( s if s == stringify!($variant).to_lowercase().as_str() => Ok($enum_name::$variant), )*
                    _ => Err(saps::errors::saps::SapsError::bad_request(
                        format!("Unknown role: {}", value)
                    )),
                }
            }
        }

        // --- 4. Implement the UserRole marker trait ---
        impl saps::auth::token::checks::UserRole for $enum_name {}

        // --- 5. Generate check structs ---
        $(
            #[derive(Clone)]
            pub struct $struct;

            impl saps::auth::token::checks::CheckUserRole for $struct {
                fn check_user_role<U: saps::auth::token::checks::UserRole>(
                    role: &U,
                ) -> Result<(), saps::errors::saps::SapsError> {
                    let role_str = role.to_string().to_lowercase();
                    let allowed: &[&str] = &[$( stringify!($role) ),+];
                    if allowed.iter().any(|r| r.to_lowercase() == role_str) {
                        Ok(())
                    } else {
                        Err(saps::errors::saps::SapsError {
                            status: saps::errors::saps::SapsErrorStatus::Unauthorized,
                            message: "Role does not have sufficient permissions".to_string(),
                        })
                    }
                }
            }
        )*
    };
}

// -- Default role enum and check structs --
//
// This generates:
//   - `pub enum DefaultRole { SuperAdmin, Admin, Customer }`
//   - All string conversion impls
//   - Hierarchical checks (higher roles inherit access):
//       SuperAdminRoleCheck  — only SuperAdmin
//       AdminRoleCheck       — SuperAdmin + Admin
//       CustomerRoleCheck    — SuperAdmin + Admin + Customer
//       NoRoleCheck          — any authenticated role passes (same as CustomerRoleCheck)
//   - Exact checks (must match the specific role):
//       ExactSuperAdminRoleCheck — SuperAdmin only
//       ExactAdminRoleCheck      — Admin only
//       ExactCustomerRoleCheck   — Customer only
construct_checks!(
    enum DefaultRole {
        SuperAdmin,
        Admin,
        Customer,
    }

    SuperAdminRoleCheck => SuperAdmin,
    AdminRoleCheck => SuperAdmin | Admin,
    CustomerRoleCheck => SuperAdmin | Admin | Customer,
    NoRoleCheck => SuperAdmin | Admin | Customer,
    ExactSuperAdminRoleCheck => SuperAdmin,
    ExactAdminRoleCheck => Admin,
    ExactCustomerRoleCheck => Customer
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::saps::SapsErrorStatus;

    // -- DefaultRole string conversion tests --

    #[test]
    fn test_default_role_to_string() {
        assert_eq!(DefaultRole::SuperAdmin.to_string(), "superadmin");
        assert_eq!(DefaultRole::Admin.to_string(), "admin");
        assert_eq!(DefaultRole::Customer.to_string(), "customer");
    }

    #[test]
    fn test_default_role_from_string_case_insensitive() {
        let role = DefaultRole::try_from("SuperAdmin".to_string()).unwrap();
        assert_eq!(role.to_string(), "superadmin");

        let role = DefaultRole::try_from("ADMIN".to_string()).unwrap();
        assert_eq!(role.to_string(), "admin");

        let role = DefaultRole::try_from("customer".to_string()).unwrap();
        assert_eq!(role.to_string(), "customer");
    }

    #[test]
    fn test_default_role_from_string_invalid() {
        let result = DefaultRole::try_from("unknown".to_string());
        assert!(result.is_err());
    }

    // -- SuperAdminRoleCheck: only "superadmin" passes --

    #[test]
    fn test_super_admin_check_passes_for_super_admin() {
        let role = DefaultRole::SuperAdmin;
        assert!(SuperAdminRoleCheck::check_user_role(&role).is_ok());
    }

    #[test]
    fn test_super_admin_check_rejects_admin() {
        let role = DefaultRole::Admin;
        let err = SuperAdminRoleCheck::check_user_role(&role).unwrap_err();
        assert_eq!(err.status, SapsErrorStatus::Unauthorized);
    }

    #[test]
    fn test_super_admin_check_rejects_customer() {
        let role = DefaultRole::Customer;
        assert!(SuperAdminRoleCheck::check_user_role(&role).is_err());
    }

    // -- AdminRoleCheck: "superadmin" and "admin" pass, "customer" is rejected --

    #[test]
    fn test_admin_check_passes_for_super_admin() {
        let role = DefaultRole::SuperAdmin;
        assert!(AdminRoleCheck::check_user_role(&role).is_ok());
    }

    #[test]
    fn test_admin_check_passes_for_admin() {
        let role = DefaultRole::Admin;
        assert!(AdminRoleCheck::check_user_role(&role).is_ok());
    }

    #[test]
    fn test_admin_check_rejects_customer() {
        let role = DefaultRole::Customer;
        assert!(AdminRoleCheck::check_user_role(&role).is_err());
    }

    // -- CustomerRoleCheck: all three roles pass (broadest hierarchical check) --

    #[test]
    fn test_customer_check_passes_for_all_roles() {
        assert!(CustomerRoleCheck::check_user_role(&DefaultRole::SuperAdmin).is_ok());
        assert!(CustomerRoleCheck::check_user_role(&DefaultRole::Admin).is_ok());
        assert!(CustomerRoleCheck::check_user_role(&DefaultRole::Customer).is_ok());
    }

    // -- NoRoleCheck: same as CustomerRoleCheck, any authenticated role passes --

    #[test]
    fn test_no_role_check_passes_for_all_roles() {
        assert!(NoRoleCheck::check_user_role(&DefaultRole::SuperAdmin).is_ok());
        assert!(NoRoleCheck::check_user_role(&DefaultRole::Admin).is_ok());
        assert!(NoRoleCheck::check_user_role(&DefaultRole::Customer).is_ok());
    }

    // -- ExactSuperAdminRoleCheck: only "superadmin", nothing else --

    #[test]
    fn test_exact_super_admin_check_passes_only_for_super_admin() {
        assert!(ExactSuperAdminRoleCheck::check_user_role(&DefaultRole::SuperAdmin).is_ok());
        assert!(ExactSuperAdminRoleCheck::check_user_role(&DefaultRole::Admin).is_err());
        assert!(ExactSuperAdminRoleCheck::check_user_role(&DefaultRole::Customer).is_err());
    }

    // -- ExactAdminRoleCheck: only "admin", nothing else --

    #[test]
    fn test_exact_admin_check_rejects_super_admin() {
        let role = DefaultRole::SuperAdmin;
        assert!(ExactAdminRoleCheck::check_user_role(&role).is_err());
    }

    #[test]
    fn test_exact_admin_check_passes_for_admin() {
        let role = DefaultRole::Admin;
        assert!(ExactAdminRoleCheck::check_user_role(&role).is_ok());
    }

    // -- ExactCustomerRoleCheck: only "customer", nothing else --

    #[test]
    fn test_exact_customer_check_passes_only_for_customer() {
        assert!(ExactCustomerRoleCheck::check_user_role(&DefaultRole::Customer).is_ok());
        assert!(ExactCustomerRoleCheck::check_user_role(&DefaultRole::Admin).is_err());
        assert!(ExactCustomerRoleCheck::check_user_role(&DefaultRole::SuperAdmin).is_err());
    }

    // -- Custom enum via construct_checks! (proves users can define their own) --

    construct_checks!(
        enum CustomRole {
            Owner,
            Editor,
            Viewer,
        }

        OwnerOnly => Owner,
        EditorOrAbove => Owner | Editor,
        AnyCustomRole => Owner | Editor | Viewer,
    );

    #[test]
    fn test_custom_role_owner_only() {
        assert!(OwnerOnly::check_user_role(&CustomRole::Owner).is_ok());
        assert!(OwnerOnly::check_user_role(&CustomRole::Editor).is_err());
        assert!(OwnerOnly::check_user_role(&CustomRole::Viewer).is_err());
    }

    #[test]
    fn test_custom_role_editor_or_above() {
        assert!(EditorOrAbove::check_user_role(&CustomRole::Owner).is_ok());
        assert!(EditorOrAbove::check_user_role(&CustomRole::Editor).is_ok());
        assert!(EditorOrAbove::check_user_role(&CustomRole::Viewer).is_err());
    }

    #[test]
    fn test_custom_role_any() {
        assert!(AnyCustomRole::check_user_role(&CustomRole::Owner).is_ok());
        assert!(AnyCustomRole::check_user_role(&CustomRole::Editor).is_ok());
        assert!(AnyCustomRole::check_user_role(&CustomRole::Viewer).is_ok());
    }

    #[test]
    fn test_custom_role_string_roundtrip() {
        let role = CustomRole::try_from("OWNER".to_string()).unwrap();
        assert_eq!(role.to_string(), "owner");

        let role = CustomRole::try_from("editor".to_string()).unwrap();
        assert_eq!(role.to_string(), "editor");
    }
}
