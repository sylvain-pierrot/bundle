//! BPv7 bundle structure (RFC 9171 §4).

pub mod canonical;
pub mod primary;

pub use canonical::{
    BlockData, BlockFlags, CanonicalBlock, PAYLOAD_BLOCK_NUMBER, PAYLOAD_BLOCK_TYPE,
};
pub use primary::{BundleFlags, CreationTimestamp, FragmentInfo, PrimaryBlock};

use alloc::vec::Vec;

use aqueduct_cbor::{Encoder, ToCbor};

use crate::crc::Crc;
use crate::error::Error;

/// A BPv7 bundle (RFC 9171 §4.1).
///
/// Pure data — holds metadata and block descriptors in memory.
/// No I/O, no retention. Constructed via buffer-based decode
/// or by the streaming reader in the `aqueduct` crate.
#[derive(Debug, Clone)]
pub struct Bundle {
    primary: PrimaryBlock,
    blocks: Vec<CanonicalBlock>,
}

impl Bundle {
    pub fn from_parts(primary: PrimaryBlock, blocks: Vec<CanonicalBlock>) -> Self {
        Bundle { primary, blocks }
    }

    pub fn primary(&self) -> &PrimaryBlock {
        &self.primary
    }

    pub fn primary_mut(&mut self) -> &mut PrimaryBlock {
        &mut self.primary
    }

    pub fn blocks(&self) -> &[CanonicalBlock] {
        &self.blocks
    }

    pub fn blocks_mut(&mut self) -> &mut Vec<CanonicalBlock> {
        &mut self.blocks
    }

    pub fn extensions(&self) -> impl Iterator<Item = &CanonicalBlock> {
        self.blocks.iter().filter(|b| !b.is_payload())
    }

    pub fn payload_block(&self) -> Option<&CanonicalBlock> {
        self.blocks.iter().find(|b| b.is_payload())
    }

    pub fn payload_len(&self) -> u64 {
        self.payload_block()
            .map(|b| match &b.data {
                BlockData::Inline(data) => data.len() as u64,
                BlockData::Retained { len, .. } => *len,
            })
            .unwrap_or(0)
    }

    pub fn payload_crc(&self) -> Crc {
        self.payload_block().map(|b| b.crc).unwrap_or(Crc::None)
    }

    pub fn block_by_type(&self, block_type: u64) -> Option<&CanonicalBlock> {
        self.blocks.iter().find(|b| b.block_type == block_type)
    }

    pub fn block_by_number(&self, number: u64) -> Option<&CanonicalBlock> {
        self.blocks.iter().find(|b| b.block_number == number)
    }

    pub fn validate(&self) -> Result<(), Error> {
        self.primary.validate()?;

        let mut payload_count = 0;
        for (i, a) in self.blocks.iter().enumerate() {
            if a.is_payload() {
                payload_count += 1;
            }
            for b in &self.blocks[i + 1..] {
                if a.block_number == b.block_number {
                    return Err(Error::DuplicateBlockNumber(a.block_number));
                }
            }
        }
        if payload_count != 1 {
            return Err(Error::InvalidPayloadCount(payload_count));
        }

        Ok(())
    }

    /// Encode the bundle to bytes. Only works when all blocks are inline.
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        self.validate()?;

        let mut enc = Encoder::with_capacity(256);
        enc.write_indefinite_array();
        self.primary.encode(&mut enc);

        for block in &self.blocks {
            match &block.data {
                BlockData::Inline(_) => block.encode(&mut enc),
                BlockData::Retained { .. } => {
                    return Err(Error::PayloadNotInline);
                }
            }
        }

        enc.write_break();
        Ok(enc.into_bytes())
    }
}
