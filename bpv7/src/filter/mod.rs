//! Bundle filtering and mutation pipeline.
//!
//! Trait definitions and built-in filters/mutators.
//! The `FilterChain` orchestrator stays in the main aqueduct crate.

pub mod builtin;
pub mod error;
pub mod traits;

pub use error::FilterRejection;
pub use traits::{BundleFilter, BundleMetadata, BundleMutator};
