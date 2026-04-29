pub mod builder;
pub mod canonical;
pub mod crc;
pub mod primary;

use std::io::Read;

use aqueduct_cbor::{Encoder, ToCbor};

use canonical::{BlockData, CanonicalBlock};
use crc::Crc;
use primary::PrimaryBlock;

use crate::error::Error;
use crate::retention::Retention;

/// A BPv7 bundle (RFC 9171 §4.1).
///
/// Pure data — holds metadata in memory and a retention backend for
/// the wire bytes. Constructed via [`BundleReader`](crate::BundleReader)
/// or [`BundleBuilder`](crate::BundleBuilder). Never constructed directly.
#[derive(Debug, Clone)]
pub struct Bundle<S> {
    primary: PrimaryBlock,
    blocks: Vec<CanonicalBlock>,
    retention: S,
}

impl<S> Bundle<S> {
    pub(crate) fn from_parts(
        primary: PrimaryBlock,
        blocks: Vec<CanonicalBlock>,
        retention: S,
    ) -> Self {
        Bundle {
            primary,
            blocks,
            retention,
        }
    }

    pub(crate) fn swap_retention<T>(self, retention: T) -> Bundle<T> {
        Bundle {
            primary: self.primary,
            blocks: self.blocks,
            retention,
        }
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
            .and_then(|b| b.retained_range())
            .map(|(_, len)| len)
            .unwrap_or(0)
    }

    pub fn payload_crc(&self) -> Crc {
        self.payload_block().map(|b| b.crc).unwrap_or(Crc::None)
    }

    pub fn retention(&self) -> &S {
        &self.retention
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
}

impl<S: Retention> Bundle<S> {
    pub fn payload_reader(&self) -> std::io::Result<S::Reader<'_>> {
        let (offset, len) = self
            .payload_block()
            .and_then(|b| b.retained_range())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no payload block"))?;
        self.retention.reader(offset, len)
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        self.validate()?;

        let mut enc = Encoder::new();
        enc.write_indefinite_array();
        self.primary.encode(&mut enc);

        for block in &self.blocks {
            match &block.data {
                BlockData::Inline(_) => block.encode(&mut enc),
                BlockData::Retained { offset, len } => {
                    let mut payload_data = Vec::with_capacity(*len as usize);
                    self.retention
                        .reader(*offset, *len)
                        .map_err(aqueduct_cbor::Error::from)?
                        .read_to_end(&mut payload_data)
                        .map_err(aqueduct_cbor::Error::from)?;

                    let has_crc = !block.crc.is_none();
                    let block_start = enc.position();
                    enc.write_array(if has_crc { 6 } else { 5 });
                    enc.write_uint(block.block_type);
                    enc.write_uint(block.block_number);
                    enc.write_uint(block.flags.bits());
                    enc.write_uint(block.crc.crc_type());
                    enc.write_bstr(&payload_data);
                    block.crc.encode_and_finalize(&mut enc, block_start);
                }
            }
        }

        enc.write_break();
        Ok(enc.into_bytes())
    }

    pub fn encode_to<W: std::io::Write>(&self, writer: W) -> Result<(), Error> {
        self.validate()?;
        use crate::io::BundleWriter;

        let mut w = BundleWriter::new(writer)?;
        w.write_primary(&self.primary)?;

        for block in &self.blocks {
            match &block.data {
                BlockData::Inline(_) => w.write_extension(block)?,
                BlockData::Retained { offset, len } => {
                    w.begin_payload(block.flags, block.crc, *len)?;
                    let mut reader = self
                        .retention
                        .reader(*offset, *len)
                        .map_err(aqueduct_cbor::Error::from)?;
                    let mut buf = [0u8; 65536];
                    loop {
                        let n = reader.read(&mut buf).map_err(aqueduct_cbor::Error::from)?;
                        if n == 0 {
                            break;
                        }
                        w.write_payload_data(&buf[..n])?;
                    }
                    w.end_payload()?;
                }
            }
        }

        w.finish()?;
        Ok(())
    }
}
