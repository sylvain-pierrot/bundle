//! Streaming bundle writer.

use std::io::Write;

use aqueduct_cbor::{Encoder, StreamEncoder, ToCbor};

use crate::bundle::canonical::{BlockFlags, CanonicalBlock};
use crate::bundle::crc::{Crc, CrcHasher};
use crate::bundle::primary::PrimaryBlock;
use crate::error::Error;

/// Streaming bundle encoder.
///
/// Blocks are encoded from in-memory structs. Payload data is written in
/// chunks via [`write_payload_data`](Self::write_payload_data) — it is never
/// buffered in full.
pub struct BundleWriter<W> {
    enc: StreamEncoder<W>,
    payload_hasher: Option<CrcHasher>,
    payload_crc: Crc,
    payload_remaining: u64,
}

impl<W: Write> BundleWriter<W> {
    /// Create a new writer, emitting the bundle's indefinite-array start.
    pub fn new(writer: W) -> Result<Self, Error> {
        let mut enc = StreamEncoder::new(writer);
        enc.write_indefinite_array()?;
        Ok(Self {
            enc,
            payload_hasher: None,
            payload_crc: Crc::None,
            payload_remaining: 0,
        })
    }

    /// Write the primary block (encoded to a temp buffer, CRC computed, then
    /// flushed to the stream).
    pub fn write_primary(&mut self, primary: &PrimaryBlock<'_>) -> Result<(), Error> {
        let mut buf = Encoder::new();
        primary.encode(&mut buf);
        self.enc.write_raw(buf.as_bytes())?;
        Ok(())
    }

    /// Write an extension block.
    pub fn write_extension(&mut self, block: &CanonicalBlock) -> Result<(), Error> {
        let mut buf = Encoder::new();
        block.encode(&mut buf);
        self.enc.write_raw(buf.as_bytes())?;
        Ok(())
    }

    /// Begin writing the payload block. After this call, use
    /// [`write_payload_data`](Self::write_payload_data) to stream the payload,
    /// then [`end_payload`](Self::end_payload) to finalize.
    pub fn begin_payload(
        &mut self,
        flags: BlockFlags,
        crc: Crc,
        data_len: u64,
    ) -> Result<(), Error> {
        let has_crc = !crc.is_none();

        // Encode the payload block header (everything before the data bytes)
        let mut header = Encoder::new();
        header.write_array(if has_crc { 6 } else { 5 });
        header.write_uint(1); // block type = payload
        header.write_uint(1); // block number
        header.write_uint(flags.bits());
        header.write_uint(crc.crc_type());
        header.write_bstr_header(data_len);

        self.enc.write_raw(header.as_bytes())?;

        // Start incremental CRC if needed
        self.payload_hasher = CrcHasher::new(&crc);
        if let Some(h) = &mut self.payload_hasher {
            h.update(header.as_bytes());
        }

        self.payload_crc = crc;
        self.payload_remaining = data_len;

        Ok(())
    }

    /// Write a chunk of payload data.
    pub fn write_payload_data(&mut self, data: &[u8]) -> Result<(), Error> {
        let len = data.len() as u64;
        if len > self.payload_remaining {
            return Err(Error::PayloadOverflow);
        }
        self.enc.write_raw(data)?;
        if let Some(h) = &mut self.payload_hasher {
            h.update(data);
        }
        self.payload_remaining -= len;
        Ok(())
    }

    /// Finalize the payload block — computes and writes the CRC if needed.
    pub fn end_payload(&mut self) -> Result<(), Error> {
        if let Some(mut hasher) = self.payload_hasher.take() {
            let crc_size = self.payload_crc.value_size();
            let mut placeholder = Encoder::new();
            placeholder.write_bstr(&vec![0u8; crc_size]);
            hasher.update(placeholder.as_bytes());

            let computed = hasher.finalize();
            let mut crc_buf = [0u8; 4];
            let n = computed.write_value(&mut crc_buf);
            self.enc.write_bstr(&crc_buf[..n])?;
        }

        Ok(())
    }

    /// Write the closing break code and flush. Returns the inner writer.
    pub fn finish(mut self) -> Result<W, Error> {
        self.enc.write_break()?;
        self.enc.flush()?;
        Ok(self.enc.into_inner())
    }
}
