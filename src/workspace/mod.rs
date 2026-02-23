#[allow(dead_code)] // Used from lib crate's api::workspaces, not directly from binary
pub mod service;
pub mod types;

#[allow(unused_imports)] // Re-exported for lib crate consumers
pub use types::*;
