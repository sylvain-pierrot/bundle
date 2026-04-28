use crate::error::Error;
use crate::extension::Extension;

/// Hop Count Block (RFC 9171 §4.4.3), block type 10.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopCount {
    pub limit: u8,
    pub count: u8,
}

impl HopCount {
    #[inline]
    pub fn exceeded(&self) -> bool {
        self.count > self.limit
    }
}

impl Extension for HopCount {
    const BLOCK_TYPE: u64 = 10;

    fn parse(_data: &[u8]) -> Result<Self, Error> {
        // TODO: delegate to external CBOR decoder
        todo!("CBOR decode for HopCount")
    }
}
