pub mod bundle;
pub mod eid;
pub mod error;
pub mod extension;
pub mod io;
pub mod retention;

#[cfg(feature = "async")]
pub use io::{AsyncRetention, BundleAsyncReader};
pub use io::{BlockEvent, BundleReader, BundleWriter, OpenBundleReader, PayloadReader};

pub use bundle::Bundle;
pub use bundle::builder::BundleBuilder;
pub use bundle::canonical::{BlockData, BlockFlags, CanonicalBlock};
pub use bundle::crc::Crc;
pub use bundle::primary::{BundleFlags, CreationTimestamp, FragmentInfo, PrimaryBlock};
pub use eid::Eid;
pub use error::Error;
pub use extension::{BundleAge, Extension, HopCount, PreviousNode};
pub use retention::{DiskRetention, MemoryRetention, Retention};
