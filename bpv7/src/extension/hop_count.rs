use alloc::vec::Vec;

use bundle_cbor::{Decoder, Encoder};

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

    fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut dec = Decoder::new(data);
        let len = dec.read_array_len()?;
        if len != 2 {
            return Err(Error::InvalidBlockLength {
                expected: "2",
                actual: len,
            });
        }
        let limit = u8::try_from(dec.read_uint()?).map_err(|_| Error::IntegerOverflow)?;
        let count = u8::try_from(dec.read_uint()?).map_err(|_| Error::IntegerOverflow)?;
        Ok(HopCount { limit, count })
    }

    fn encode_data(&self) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.write_array(2);
        enc.write_uint(self.limit as u64);
        enc.write_uint(self.count as u64);
        enc.into_bytes()
    }
}
