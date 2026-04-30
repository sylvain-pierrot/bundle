#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

pub mod bundle;
pub mod filter;
pub mod io;
pub mod retention;

pub use bundle::Bundle;
pub use bundle::builder::BundleBuilder;
pub use filter::FilterChain;
pub use io::{BlockEvent, BundleReader, BundleWriter, OpenBundleReader, PayloadReader};
pub use retention::{MemoryRetention, Retention};

#[cfg(feature = "std")]
pub use retention::DiskRetention;

#[cfg(feature = "async")]
pub use io::{BundleAsyncReader, BundleAsyncWriter};
#[cfg(feature = "async")]
pub use retention::{AsyncRetention, S3Ops, S3Retention};
