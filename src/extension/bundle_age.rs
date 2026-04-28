use aqueduct_cbor::{Decoder, Encoder};

use crate::error::Error;
use crate::extension::Extension;

/// Bundle Age Block (RFC 9171 §4.4.2), block type 7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleAge {
    pub millis: u64,
}

impl Extension for BundleAge {
    const BLOCK_TYPE: u64 = 7;

    fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut dec = Decoder::new(data);
        let millis = dec.read_uint()?;
        Ok(BundleAge { millis })
    }

    fn encode_data(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.write_uint(self.millis);
        enc.into_bytes()
    }
}
