//! Streaming bundle writer.

use std::io::Write;

use aqueduct_cbor::{Encoder, StreamEncoder, ToCbor};

use crate::bundle::canonical::{BlockFlags, CanonicalBlock};
use crate::bundle::crc::{Crc, CrcHasher};
use crate::bundle::primary::PrimaryBlock;
use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Init,
    Blocks,
    Payload,
    Done,
}

/// Streaming bundle encoder.
pub struct BundleWriter<W> {
    enc: StreamEncoder<W>,
    state: State,
    payload_hasher: Option<CrcHasher>,
    payload_crc: Crc,
    payload_remaining: u64,
}

impl<W: Write> BundleWriter<W> {
    pub fn new(writer: W) -> Result<Self, Error> {
        let mut enc = StreamEncoder::new(writer);
        enc.write_indefinite_array()?;
        Ok(Self {
            enc,
            state: State::Init,
            payload_hasher: None,
            payload_crc: Crc::None,
            payload_remaining: 0,
        })
    }

    pub fn write_primary(&mut self, primary: &PrimaryBlock) -> Result<(), Error> {
        let mut buf = Encoder::new();
        primary.encode(&mut buf);
        self.enc.write_raw(buf.as_bytes())?;
        self.state = State::Blocks;
        Ok(())
    }

    pub fn write_extension(&mut self, block: &CanonicalBlock) -> Result<(), Error> {
        let mut buf = Encoder::new();
        block.encode(&mut buf);
        self.enc.write_raw(buf.as_bytes())?;
        Ok(())
    }

    pub fn begin_payload(
        &mut self,
        flags: BlockFlags,
        crc: Crc,
        data_len: u64,
    ) -> Result<(), Error> {
        let has_crc = !crc.is_none();

        let mut header = Encoder::new();
        header.write_array(if has_crc { 6 } else { 5 });
        header.write_uint(1);
        header.write_uint(1);
        header.write_uint(flags.bits());
        header.write_uint(crc.crc_type());
        header.write_bstr_header(data_len);

        self.enc.write_raw(header.as_bytes())?;

        self.payload_hasher = CrcHasher::new(&crc);
        if let Some(h) = &mut self.payload_hasher {
            h.update(header.as_bytes());
        }

        self.payload_crc = crc;
        self.payload_remaining = data_len;
        self.state = State::Payload;
        Ok(())
    }

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

    pub fn end_payload(&mut self) -> Result<(), Error> {
        if let Some(mut hasher) = self.payload_hasher.take() {
            let crc_size = self.payload_crc.value_size();
            // CBOR bstr header (1 byte) + zeroed CRC value
            let mut zeroed = [0u8; 5]; // max: 1 header + 4 value
            zeroed[0] = 0x40 | crc_size as u8; // CBOR bstr major type 2, length < 24
            hasher.update(&zeroed[..1 + crc_size]);

            let computed = hasher.finalize();
            let mut crc_buf = [0u8; 4];
            let n = computed.write_value(&mut crc_buf);
            self.enc.write_bstr(&crc_buf[..n])?;
        }
        self.state = State::Blocks;
        Ok(())
    }

    pub fn finish(mut self) -> Result<W, Error> {
        self.enc.write_break()?;
        self.enc.flush()?;
        self.state = State::Done;
        Ok(self.enc.into_inner())
    }
}
