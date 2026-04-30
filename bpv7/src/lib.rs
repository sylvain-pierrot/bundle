//! BPv7 bundle protocol types (RFC 9171).
//!
//! Core data types for Bundle Protocol Version 7. No I/O, no runtime
//! dependencies. Works in `no_std + alloc` environments.

#![no_std]
extern crate alloc;

pub mod bundle;
pub mod crc;
pub mod eid;
pub mod error;
pub mod extension;
pub mod filter;

pub use bundle::{
    BlockData, BlockFlags, Bundle, BundleFlags, CanonicalBlock, CreationTimestamp, FragmentInfo,
    PAYLOAD_BLOCK_NUMBER, PAYLOAD_BLOCK_TYPE, PrimaryBlock,
};
pub use crc::{Crc, CrcHasher};
pub use eid::Eid;
pub use error::Error;
pub use extension::{BundleAge, Extension, HopCount, PreviousNode};
pub use filter::{BundleFilter, BundleMetadata, BundleMutator, FilterRejection};
