pub mod canonical;
pub mod crc;
pub mod primary;

use canonical::{BlockFlags, CanonicalBlock};
use crc::Crc;
use primary::PrimaryBlock;

use crate::error::Error;

/// Coordinates of the payload data within the original input.
///
/// The payload bytes are never held by the bundle — they stay wherever the
/// caller stored them (S3, file, memory). This struct records where to find them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadRef {
    pub flags: BlockFlags,
    pub crc: Crc,
    /// Byte offset in the original input where payload data starts.
    pub data_offset: u64,
    /// Length of the payload data in bytes.
    pub data_len: u64,
}

/// A BPv7 bundle (RFC 9171 §4.1).
///
/// Holds the bundle map: metadata is owned in memory, the payload is
/// represented as an offset + length into the original input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle<'a> {
    pub primary: PrimaryBlock<'a>,
    pub extensions: Vec<CanonicalBlock>,
    pub payload: PayloadRef,
}

impl Bundle<'_> {
    #[inline]
    pub fn block_by_type(&self, block_type: u64) -> Option<&CanonicalBlock> {
        self.extensions.iter().find(|b| b.block_type == block_type)
    }

    #[inline]
    pub fn block_by_number(&self, number: u64) -> Option<&CanonicalBlock> {
        self.extensions.iter().find(|b| b.block_number == number)
    }

    pub fn validate(&self) -> Result<(), Error> {
        self.primary.validate()?;

        // Payload block number is always 1 (RFC 9171 §4.3.3).
        const PAYLOAD_BLOCK_NUMBER: u64 = 1;

        for (i, a) in self.extensions.iter().enumerate() {
            if a.block_number == PAYLOAD_BLOCK_NUMBER {
                return Err(Error::DuplicateBlockNumber(a.block_number));
            }
            for b in &self.extensions[i + 1..] {
                if a.block_number == b.block_number {
                    return Err(Error::DuplicateBlockNumber(a.block_number));
                }
            }
        }

        Ok(())
    }
}
