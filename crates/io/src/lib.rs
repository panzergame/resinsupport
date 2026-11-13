//! L3: reading model files; reading and writing project files; writing printer files. Every file is hostile input: errors are values, never panics.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod ctb;
pub mod project;
#[cfg(not(target_arch = "wasm32"))]
pub mod safe_write;
pub mod stl;

pub use ctb::{CtbFile, CtbOptions, read_ctb, write_ctb, write_ctb_with};
#[cfg(not(target_arch = "wasm32"))]
pub use project::read_project_file;
pub use project::{Project, ProjectModel, ProjectSupport, read_project, write_project};
#[cfg(not(target_arch = "wasm32"))]
pub use safe_write::write_atomic;
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
    #[error("cancelled")]
    Cancelled,
}
