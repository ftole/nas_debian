//! Asistente TUI del servidor NAS.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod app;
pub mod ui;

pub use app::{App, Data, Section};
