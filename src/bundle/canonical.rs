use aqueduct_cbor::{Decoder, Encoder, FromCbor, ToCbor};

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
    /// Decode the body of a canonical block after block_type and array_len
    /// have already been read.
    pub(crate) fn decode_body(
        dec: &mut Decoder<'_>,
        block_type: u64,
        array_len: usize,
    ) -> Result<Self, Error> {
        if array_len != 5 && array_len != 6 {
            return Err(Error::InvalidBlockLength {
                expected: "5-6",
                actual: array_len,
            });
        }
        let block_number = dec.read_uint()?;
        let flags = BlockFlags::from_bits(dec.read_uint()?);
        let crc_type = dec.read_uint()?;
        let data = dec.read_bstr()?.to_vec();

        let crc = if crc_type != 0 {
            Crc::decode_value(dec, crc_type)?
        } else {
            Crc::None
        };

        Ok(CanonicalBlock {
            block_type,
            block_number,
            flags,
            crc,
            data,
        })
    }

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

    /// Create a canonical block from an extension type.
    pub fn from_ext<E: Extension>(block_number: u64, flags: BlockFlags, crc: Crc, ext: &E) -> Self {
        CanonicalBlock {
            block_type: E::BLOCK_TYPE,
            block_number,
            flags,
            crc,
            data: ext.encode_data(),
        }
    }
}

impl FromCbor<'_> for CanonicalBlock {
    type Error = Error;

    fn decode(dec: &mut Decoder<'_>) -> Result<Self, Self::Error> {
        let array_len = dec.read_array_len()?;
        let block_type = dec.read_uint()?;
        Self::decode_body(dec, block_type, array_len)
    }
}

impl ToCbor for CanonicalBlock {
    fn encode(&self, enc: &mut Encoder) {
        let has_crc = !self.crc.is_none();
        let block_start = enc.position();
        enc.write_array(if has_crc { 6 } else { 5 });
        enc.write_uint(self.block_type);
        enc.write_uint(self.block_number);
        enc.write_uint(self.flags.bits());
        enc.write_uint(self.crc.crc_type());
        enc.write_bstr(&self.data);

        self.crc.encode_and_finalize(enc, block_start);
    }
}
