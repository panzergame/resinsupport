//! L3: reading model files. Every file is hostile input: errors are values, never panics.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod stl;

#[cfg(not(target_arch = "wasm32"))]
pub use stl::read_stl_file;
pub use stl::{StlImport, read_stl};

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("bad file: {0}")]
    Format(String),
    #[error("file too large: {0}")]
    TooLarge(String),
}
