//! Manejadores HTTP del panel web.

pub mod audit;
pub mod auth;
pub mod backups;
pub mod common;
pub mod diagnostics;
pub mod domain;
pub mod files;
pub mod logs;
pub mod pages;
pub mod preview;
pub mod services;
pub mod shares;
pub mod storage;
pub mod system;
pub mod terminal;
pub mod trash;
pub mod users;

#[cfg(test)]
mod tests;

pub use audit::*;
pub use auth::*;
pub use backups::*;
pub use common::*;
pub use diagnostics::*;
pub use domain::*;
pub use files::*;
pub use logs::*;
pub use pages::*;
pub use preview::*;
pub use services::*;
pub use shares::*;
pub use system::*;
pub use terminal::*;
pub use trash::*;
pub use users::*;
