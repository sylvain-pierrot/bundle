use crate::bundle::crc::Crc;
use crate::error::Error;
use crate::extension::Extension;

/// Block processing control flags (RFC 9171 §4.2.4).
///
/// Zero-cost newtype over `u64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockFlags(u64);

impl BlockFlags {
    #[inline]
    pub fn from_bits(bits: u64) -> Self {
        Self(bits)
    }
    #[inline]
    pub fn bits(self) -> u64 {
        self.0
    }
    #[inline]
    pub fn must_replicate(self) -> bool {
        self.0 & 0x01 != 0
    }
    #[inline]
    pub fn report_on_failure(self) -> bool {
        self.0 & 0x02 != 0
    }
    #[inline]
    pub fn delete_bundle_on_failure(self) -> bool {
        self.0 & 0x04 != 0
    }
    #[inline]
    pub fn discard_on_failure(self) -> bool {
        self.0 & 0x10 != 0
    }
}

/// Canonical bundle block (RFC 9171 §4.3.2).
///
/// Used for extension blocks. Data is owned — extension blocks are always small.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalBlock {
    pub block_type: u64,
    pub block_number: u64,
    pub flags: BlockFlags,
    pub crc: Crc,
    pub data: Vec<u8>,
}

impl CanonicalBlock {
    /// Parse block data as extension type `E`.
    ///
    /// Returns `Err(BlockTypeMismatch)` if `self.block_type != E::BLOCK_TYPE`.
    pub fn parse_ext<E: Extension>(&self) -> Result<E, Error> {
        if self.block_type != E::BLOCK_TYPE {
            return Err(Error::BlockTypeMismatch {
                expected: E::BLOCK_TYPE,
                actual: self.block_type,
            });
        }
        E::parse(&self.data)
    }
}
