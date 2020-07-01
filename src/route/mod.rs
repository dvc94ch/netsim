use std::io;
use thiserror::Error;

mod v4;
mod v6;

pub use self::v4::*;
pub use self::v6::*;

/// Errors returned by `add_route` and `Route::add`
#[derive(Debug, Error)]
pub enum AddRouteError {
    /// Process file descriptor limit hit
    #[error("process file descriptor limit hit ({0})")]
    ProcessFileDescriptorLimit(io::Error),
    /// System file descriptor limit hit
    #[error("system file descriptor limit hit ({0})")]
    SystemFileDescriptorLimit(io::Error),
    /// Interface name contains an interior NUL byte
    #[error("interface name contains interior NUL byte")]
    NameContainsNul,
}
