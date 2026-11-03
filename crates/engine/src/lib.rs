//! L4: the session and its command registry. Everything the user can do is a command run through
//! [`Session::execute`] with JSON parameters, so the UI, tests and future scripting reach the
//! same code. Programmatic calls never open dialogs.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod cmd;
mod session;

pub use cmd::{CommandSpec, commands};
pub use resin_geom as geom;
pub use resin_render as render;
pub use session::{MAX_EXTENT, Model, SCALE_RANGE, Scene, Session, Support};

pub type Result<T, E = EngineError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    #[error("bad parameters: {0}")]
    BadParams(String),
    #[error("{0}")]
    Failed(String),
    #[error(transparent)]
    Io(#[from] resin_io::IoError),
    #[error("internal error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests;
