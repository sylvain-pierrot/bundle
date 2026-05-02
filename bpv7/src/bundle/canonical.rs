use alloc::vec::Vec;

use bundle_cbor::{Decoder, Encoder, ToCbor};

use crate::crc::Crc;
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

    /// Decode from a buffer-based CBOR decoder.
    ///
    /// Extension blocks are fully read. Payload blocks read only the
    /// bstr header — the data follows in the buffer. Returns the block
    /// and a flag indicating whether payload data follows.
    pub fn decode_buf(dec: &mut Decoder<'_>) -> Result<(Self, bool), Error> {
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
            let (data, offset) = dec.read_bstr_with_offset()?;
            let data_len = data.len() as u64;
            let crc = if crc_type != 0 {
                Crc::decode_buf(dec, crc_type)?
            } else {
                Crc::None
            };
            let block = CanonicalBlock {
                block_type,
                block_number,
                flags,
                crc,
                data: BlockData::Retained {
                    offset: offset as u64,
                    len: data_len,
                },
            };
            Ok((block, true))
        } else {
            let data = dec.read_bstr()?;
            let crc = if crc_type != 0 {
                Crc::decode_buf(dec, crc_type)?
            } else {
                Crc::None
            };
            let block = CanonicalBlock {
                block_type,
                block_number,
                flags,
                crc,
                data: BlockData::Inline(data.to_vec()),
            };
            Ok((block, false))
        }
    }

    /// Decode from a streaming CBOR decoder.
    pub fn decode_stream<R: bundle_cbor::Read>(
        dec: &mut bundle_cbor::StreamDecoder<R>,
    ) -> Result<(Self, bool), Error> {
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
                Crc::decode_stream(dec, crc_type)?
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

    /// Verify the CRC of an inline (extension) block.
    ///
    /// Re-encodes the block, zeros the CRC field, and compares.
    /// Returns `Ok(())` for blocks with no CRC.
    pub fn verify_crc(&self) -> Result<(), Error> {
        if self.crc.is_none() {
            return Ok(());
        }
        let mut enc = Encoder::with_capacity(64);
        self.encode(&mut enc);
        let bytes = enc.as_bytes();
        // CRC bstr is the last field. Find its data offset.
        // The CRC bstr is at the end: header(1) + value(2 or 4).
        let crc_size = self.crc.value_size();
        let crc_data_offset = bytes.len() - crc_size;
        self.crc.verify(bytes, crc_data_offset)
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
            BlockData::Retained { .. } => Err(Error::PayloadNotInline),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_extension_block() {
        let block = CanonicalBlock {
            block_type: 7,
            block_number: 2,
            flags: BlockFlags::from_bits(0),
            crc: Crc::None,
            data: BlockData::Inline(alloc::vec![0x18, 0x64]), // CBOR uint 100
        };
        let mut enc = Encoder::new();
        block.encode(&mut enc);
        let mut dec = Decoder::new(enc.as_bytes());
        let (decoded, is_payload) = CanonicalBlock::decode_buf(&mut dec).unwrap();
        assert!(!is_payload);
        assert_eq!(decoded, block);
    }

    #[test]
    fn roundtrip_extension_with_crc() {
        let block = CanonicalBlock {
            block_type: 10,
            block_number: 3,
            flags: BlockFlags::from_bits(0),
            crc: Crc::crc16(),
            data: BlockData::Inline(alloc::vec![0x82, 0x1E, 0x05]),
        };
        let mut enc = Encoder::new();
        block.encode(&mut enc);
        let mut dec = Decoder::new(enc.as_bytes());
        let (decoded, is_payload) = CanonicalBlock::decode_buf(&mut dec).unwrap();
        assert!(!is_payload);
        decoded.verify_crc().unwrap();
    }

    #[test]
    fn payload_block_detected() {
        let block = CanonicalBlock {
            block_type: PAYLOAD_BLOCK_TYPE,
            block_number: PAYLOAD_BLOCK_NUMBER,
            flags: BlockFlags::from_bits(0),
            crc: Crc::None,
            data: BlockData::Inline(alloc::vec![0xDE, 0xAD]),
        };
        assert!(block.is_payload());
    }
}
