//! metria-traffic: 零侵入流量估算（字节统计、重建、Token Profile、置信区间）。
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod error;
pub mod estimate;
pub mod profile;

pub use error::{Result, TrafficError};
pub use estimate::{estimate, estimate_with_candidates, EstimateInput, EstimateOutput};
pub use profile::{
    best_profile, builtin_bytes_per_token, builtin_candidates, builtin_profile, validate_profile,
    MatchRequest, ProfileMatch, BUILTIN_FIXED_REQUEST_BYTES, BUILTIN_FIXED_RESPONSE_BYTES,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
