use std::io::Read;

use aqueduct_cbor::{Encoder, StreamDecoder, ToCbor};

use crate::bundle::crc::Crc;
use crate::error::Error;
use crate::extension::Extension;

/// Block processing control flags (RFC 9171 §4.2.4).
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

/// Block-type-specific data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockData {
    /// Data is fully in memory (extension blocks).
    Inline(Vec<u8>),
    /// Data lives in the retention backend at the given range (payload block).
    Retained { offset: u64, len: u64 },
}

/// Canonical bundle block (RFC 9171 §4.3.2).
///
/// Represents both extension blocks and the payload block. The data
/// is either [`Inline`](BlockData::Inline) (extensions, always small) or
/// [`Retained`](BlockData::Retained) (payload, stored in the retention backend).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalBlock {
    pub block_type: u64,
    pub block_number: u64,
    pub flags: BlockFlags,
    pub crc: Crc,
    pub data: BlockData,
}

/// Payload block type code (RFC 9171 §4.3.3).
pub const PAYLOAD_BLOCK_TYPE: u64 = 1;

/// Payload block number (RFC 9171 §4.3.3).
pub const PAYLOAD_BLOCK_NUMBER: u64 = 1;

impl CanonicalBlock {
    pub fn is_payload(&self) -> bool {
        self.block_type == PAYLOAD_BLOCK_TYPE
    }

    /// Decode the next canonical block from a stream.
    ///
    /// Extension blocks are fully read. Payload blocks read only the
    /// bstr header — the data follows in the stream. The caller must
    /// consume or walk past the payload data before calling this again.
    pub(crate) fn decode<R: Read>(dec: &mut StreamDecoder<R>) -> Result<(Self, bool), Error> {
        let array_len = dec.read_array_len()?;
        if array_len != 5 && array_len != 6 {
            return Err(Error::InvalidBlockLength {
                expected: "5-6",
                actual: array_len,
            });
        }

        let block_type = dec.read_uint()?;
        let block_number = dec.read_uint()?;
        let flags = BlockFlags::from_bits(dec.read_uint()?);
        let crc_type = dec.read_uint()?;

        if block_type == PAYLOAD_BLOCK_TYPE {
            let data_len = dec.read_bstr_header()?;
            let offset = dec.position();
            let block = CanonicalBlock {
                block_type,
                block_number,
                flags,
                crc: if crc_type != 0 {
                    Crc::placeholder(crc_type)?
                } else {
                    Crc::None
                },
                data: BlockData::Retained {
                    offset,
                    len: data_len,
                },
            };
            // true = payload data follows in stream
            Ok((block, true))
        } else {
            let data = dec.read_bstr()?;
            let crc = if crc_type != 0 {
                Crc::decode(dec, crc_type)?
            } else {
                Crc::None
            };
            let block = CanonicalBlock {
                block_type,
                block_number,
                flags,
                crc,
                data: BlockData::Inline(data),
            };
            Ok((block, false))
        }
    }

    pub fn parse_ext<E: Extension>(&self) -> Result<E, Error> {
        if self.block_type != E::BLOCK_TYPE {
            return Err(Error::BlockTypeMismatch {
                expected: E::BLOCK_TYPE,
                actual: self.block_type,
            });
        }
        match &self.data {
            BlockData::Inline(data) => E::parse(data),
            BlockData::Retained { .. } => Err(Error::BlockTypeMismatch {
                expected: E::BLOCK_TYPE,
                actual: self.block_type,
            }),
        }
    }

    pub fn from_ext<E: Extension>(block_number: u64, flags: BlockFlags, crc: Crc, ext: &E) -> Self {
        CanonicalBlock {
            block_type: E::BLOCK_TYPE,
            block_number,
            flags,
            crc,
            data: BlockData::Inline(ext.encode_data()),
        }
    }

    pub fn inline_data(&self) -> Option<&[u8]> {
        match &self.data {
            BlockData::Inline(data) => Some(data),
            BlockData::Retained { .. } => None,
        }
    }

    pub fn retained_range(&self) -> Option<(u64, u64)> {
        match &self.data {
            BlockData::Inline(_) => None,
            BlockData::Retained { offset, len } => Some((*offset, *len)),
        }
    }
}

impl ToCbor for CanonicalBlock {
    fn encode(&self, enc: &mut Encoder) {
        let data = match &self.data {
            BlockData::Inline(data) => data.as_slice(),
            BlockData::Retained { .. } => {
                // Retained blocks are encoded via BundleWriter streaming path,
                // not via ToCbor. This branch should not be reached in normal use.
                return;
            }
        };

        let has_crc = !self.crc.is_none();
        let block_start = enc.position();
        enc.write_array(if has_crc { 6 } else { 5 });
        enc.write_uint(self.block_type);
        enc.write_uint(self.block_number);
        enc.write_uint(self.flags.bits());
        enc.write_uint(self.crc.crc_type());
        enc.write_bstr(data);

        self.crc.encode_and_finalize(enc, block_start);
    }
}
