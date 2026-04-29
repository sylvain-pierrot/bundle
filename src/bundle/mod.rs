pub mod builder;
pub mod canonical;
pub mod crc;
pub mod payload;
pub mod primary;

use std::io::Read;

use aqueduct_cbor::{Encoder, ToCbor};

use canonical::CanonicalBlock;
pub use payload::PayloadRef;
use primary::PrimaryBlock;

use crate::error::Error;
use crate::io::retention::Retention;
use crate::{BundleReader, Eid};

/// A BPv7 bundle (RFC 9171 §4.1).
#[derive(Debug, Clone)]
pub struct Bundle<R: Retention> {
    primary: PrimaryBlock<'static>,
    extensions: Vec<CanonicalBlock>,
    payload: PayloadRef,
    retention: R,
}

impl<R: Retention> Bundle<R> {
    pub fn builder(
        dest_eid: Eid<'_>,
        src_node_id: Eid<'_>,
        lifetime: u64,
        payload: &[u8],
        retention: R,
    ) -> builder::BundleBuilder<R> {
        builder::BundleBuilder::new(
            dest_eid.into_owned(),
            src_node_id.into_owned(),
            lifetime,
            payload,
            retention,
        )
    }

    pub fn from_bytes(data: &[u8], retention: R) -> Result<Self, Error> {
        Self::from_stream(data, retention)
    }

    pub fn from_stream<S: Read>(source: S, retention: R) -> Result<Self, Error> {
        BundleReader::new(source, retention)?.into_bundle()
    }

    pub(crate) fn from_parts(
        primary: PrimaryBlock<'static>,
        extensions: Vec<CanonicalBlock>,
        payload: PayloadRef,
        retention: R,
    ) -> Self {
        Bundle {
            primary,
            extensions,
            payload,
            retention,
        }
    }

    pub fn primary(&self) -> &PrimaryBlock<'static> {
        &self.primary
    }

    pub fn primary_mut(&mut self) -> &mut PrimaryBlock<'static> {
        &mut self.primary
    }

    pub fn extensions(&self) -> &[CanonicalBlock] {
        &self.extensions
    }

    pub fn extensions_mut(&mut self) -> &mut Vec<CanonicalBlock> {
        &mut self.extensions
    }

    pub fn payload(&self) -> &PayloadRef {
        &self.payload
    }

    pub fn retention(&self) -> &R {
        &self.retention
    }

    pub fn payload_reader(&self) -> std::io::Result<R::Reader<'_>> {
        self.retention
            .reader(self.payload.data_offset, self.payload.data_len)
    }

    pub fn block_by_type(&self, block_type: u64) -> Option<&CanonicalBlock> {
        self.extensions.iter().find(|b| b.block_type == block_type)
    }

    pub fn block_by_number(&self, number: u64) -> Option<&CanonicalBlock> {
        self.extensions.iter().find(|b| b.block_number == number)
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        let mut payload_data = Vec::with_capacity(self.payload.data_len as usize);
        self.payload_reader()
            .map_err(aqueduct_cbor::Error::from)?
            .read_to_end(&mut payload_data)
            .map_err(aqueduct_cbor::Error::from)?;

        let mut enc = Encoder::new();
        enc.write_indefinite_array();
        self.primary.encode(&mut enc);
        for ext in &self.extensions {
            ext.encode(&mut enc);
        }

        let has_crc = !self.payload.crc.is_none();
        let block_start = enc.position();
        enc.write_array(if has_crc { 6 } else { 5 });
        enc.write_uint(1);
        enc.write_uint(1);
        enc.write_uint(self.payload.flags.bits());
        enc.write_uint(self.payload.crc.crc_type());
        enc.write_bstr(&payload_data);
        self.payload.crc.encode_and_finalize(&mut enc, block_start);
        enc.write_break();
        Ok(enc.into_bytes())
    }

    pub fn validate(&self) -> Result<(), Error> {
        self.primary.validate()?;

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
