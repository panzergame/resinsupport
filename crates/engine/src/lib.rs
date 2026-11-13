//! L4: the session and its command registry. Everything the user can do is a command run through
//! [`Session::execute`] with JSON parameters, so the UI, tests and future scripting reach the
//! same code. Programmatic calls never open dialogs.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod brace;
mod cmd;
mod print_config;
mod route;
mod session;
mod slice_job;
mod support_line;
mod support_settings;
mod support_solids;

pub use brace::{
    ANGLE_RANGE, Brace, BracePairing, BraceParams, BracePattern, DEFAULT_MAX_DISTANCE, DIAMETER_RATIO_RANGE, GAP_RANGE, MAX_BAYS, MAX_BRACES,
    MAX_DISTANCE_RANGE, brace_pairs, brace_solids, brace_struts, clear_struts,
};
pub use cmd::{CommandSpec, commands};
pub use print_config::PrintConfig;
pub use resin_geom as geom;
pub use resin_io as io;
pub use resin_render as render;
pub use resin_slice as slice;
pub use route::{Route, route, support_head};
pub use session::{DEFAULT_LIFT_HEIGHT, LIFT_RANGE, MAX_EXTENT, Model, SCALE_RANGE, Scene, Session, Support, SupportPart};
pub use slice_job::{Job, SliceSource};
pub use support_line::{DEFAULT_LINE_SPACING, DEFAULT_TIPS_PER_BODY, LinePoint, MAX_LINE_TIPS, line_points};
pub use support_settings::{Base, Body, BodyShape, ContactShape, GROUPS, PresetId, Raft, SEGMENTS, SupportPreset, SupportSettings, Tip, range};
pub use support_solids::{Owner, Primitive, SupportTessellator, strut_primitives, support_primitives};

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
