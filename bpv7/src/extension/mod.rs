mod bundle_age;
mod hop_count;
mod previous_node;

pub use bundle_age::BundleAge;
pub use hop_count::HopCount;
pub use previous_node::PreviousNode;

use alloc::vec::Vec;

use crate::error::Error;

/// Extension block: parse and encode block-type-specific data.
///
/// The `parse` method receives the already-extracted block-type-specific
/// data bytes (CBOR). The `encode_data` method produces those bytes.
pub trait Extension: Sized {
    /// Block type code for this extension.
    const BLOCK_TYPE: u64;

    /// Parse block-type-specific data from raw bytes.
    fn parse(data: &[u8]) -> Result<Self, Error>;

    /// Encode block-type-specific data to raw bytes.
    fn encode_data(&self) -> Vec<u8>;
}
