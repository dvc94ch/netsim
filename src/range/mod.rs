use std::net::AddrParseError;
use std::num::ParseIntError;
use thiserror::Error;

mod v4;
mod v6;

pub use self::v4::*;
pub use self::v6::*;

/// Errors returned by `SubnetV*::from_str`
#[derive(Debug, Error)]
pub enum IpRangeParseError {
    /// Missing '/' delimiter
    #[error("missing '/' delimiter")]
    MissingDelimiter,
    /// More than one '/' delimiter
    #[error("more than one '/' delimiter")]
    ExtraDelimiter,
    /// error parsing IP address
    #[error("error parsing IP address: {0}")]
    ParseAddr(AddrParseError),
    /// error parsing netmask prefix length
    #[error("error parsing netmask prefix length: {0}")]
    ParseNetmaskPrefixLength(ParseIntError),
}
