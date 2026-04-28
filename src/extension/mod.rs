mod bundle_age;
mod hop_count;
mod previous_node;

pub use bundle_age::BundleAge;
pub use hop_count::HopCount;
pub use previous_node::PreviousNode;

use crate::error::Error;

/// Extension block: parse block-type-specific data from raw bytes.
///
/// CBOR decoding is delegated to an external crate. The `parse` method
/// receives the already-extracted block-type-specific data bytes.
pub trait Extension: Sized {
    /// Block type code for this extension.
    const BLOCK_TYPE: u64;

    /// Parse block-type-specific data from raw bytes.
    fn parse(data: &[u8]) -> Result<Self, Error>;
}
