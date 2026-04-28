pub mod builder;
pub mod canonical;
pub mod crc;
pub mod payload;
pub mod primary;

use builder::BundleBuilder;
use canonical::{BlockFlags, CanonicalBlock};
use crc::Crc;
pub use payload::PayloadRef;
use primary::PrimaryBlock;

use aqueduct_cbor::{Decoder, Encoder, FromCbor, ToCbor};

use crate::Eid;
use crate::error::Error;

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

impl<'a> Bundle<'a> {
    /// Start building a bundle with the required fields.
    pub fn builder(
        dest_eid: Eid<'a>,
        src_node_id: Eid<'a>,
        lifetime: u64,
        payload: &'a [u8],
    ) -> BundleBuilder<'a> {
        BundleBuilder::new(dest_eid, src_node_id, lifetime, payload)
    }

    /// Decode a bundle from a byte slice.
    pub fn decode(data: &'a [u8]) -> Result<Self, Error> {
        let mut dec = Decoder::new(data);
        dec.read_indefinite_array_start()?;

        let primary = PrimaryBlock::decode(&mut dec)?;

        let mut extensions = Vec::new();
        let mut payload = None;

        while !dec.is_break()? {
            let array_len = dec.read_array_len()?;
            let block_type = dec.read_uint()?;

            if block_type == 1 {
                // Payload block (RFC 9171 §4.3.3)
                if payload.is_some() {
                    return Err(Error::InvalidPayloadCount(2));
                }
                if array_len != 5 && array_len != 6 {
                    return Err(Error::InvalidCbor);
                }
                let _block_number = dec.read_uint()?;
                let flags = BlockFlags::from_bits(dec.read_uint()?);
                let crc_type = dec.read_uint()?;
                let (payload_data, data_offset) = dec.read_bstr_with_offset()?;

                let crc = if crc_type != 0 {
                    Crc::decode_value(&mut dec, crc_type)?
                } else {
                    Crc::None
                };

                payload = Some(PayloadRef {
                    flags,
                    crc,
                    data_offset: data_offset as u64,
                    data_len: payload_data.len() as u64,
                });
            } else {
                let ext = CanonicalBlock::decode_body(&mut dec, block_type, array_len)?;
                extensions.push(ext);
            }
        }

        dec.read_break()?;

        let payload = payload.ok_or(Error::InvalidPayloadCount(0))?;

        Ok(Bundle {
            primary,
            extensions,
            payload,
        })
    }
}

impl Bundle<'_> {
    /// Encode the bundle to bytes, computing CRCs.
    ///
    /// `payload_data` is the raw payload bytes (the caller owns the data;
    /// the bundle only stores coordinates via `PayloadRef`).
    pub fn encode(&self, payload_data: &[u8]) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.write_indefinite_array();

        self.primary.encode(&mut enc);

        for ext in &self.extensions {
            ext.encode(&mut enc);
        }

        // Payload block
        let has_crc = !self.payload.crc.is_none();
        let block_start = enc.position();
        enc.write_array(if has_crc { 6 } else { 5 });
        enc.write_uint(1); // block type = payload
        enc.write_uint(1); // block number (always 1)
        enc.write_uint(self.payload.flags.bits());
        enc.write_uint(self.payload.crc.crc_type());
        enc.write_bstr(payload_data);
        self.payload.crc.encode_and_finalize(&mut enc, block_start);

        enc.write_break();
        enc.into_bytes()
    }

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
