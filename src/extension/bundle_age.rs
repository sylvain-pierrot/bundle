use crate::error::Error;
use crate::extension::Extension;

/// Bundle Age Block (RFC 9171 §4.4.2), block type 7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleAge {
    pub millis: u64,
}

impl Extension for BundleAge {
    const BLOCK_TYPE: u64 = 7;

    fn parse(_data: &[u8]) -> Result<Self, Error> {
        // TODO: delegate to external CBOR decoder
        todo!("CBOR decode for BundleAge")
    }
}
