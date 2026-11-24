//! Helpers para gestión validada de cuentas del sistema y de Samba.

mod groups;
mod manage;
mod samba;
mod validation;

#[cfg(test)]
mod tests;

pub use groups::{
    accounts_status, add_group_member, create_group, delete_group, remove_group_member,
    rename_group,
};
pub use manage::{create_user, delete_user, toggle_user, update_user};
pub(crate) use samba::parse_pdbedit_w;
pub use validation::{
    ensure_manageable_existing_user, is_superadmin_account, is_valid_group, is_valid_user,
    sanitize_gecos, validate_password, CreateUserArgs, UpdateUserArgs, PROTECTED_GROUPS,
    RESERVED_USERS,
};
