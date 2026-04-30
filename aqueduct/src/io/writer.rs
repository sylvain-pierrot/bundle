//! Streaming bundle writer with optional egress filter pipeline.

use alloc::sync::Arc;
use alloc::vec::Vec;

use aqueduct_bpv7::{BlockFlags, CanonicalBlock, Crc, CrcHasher, Error, PrimaryBlock};
use aqueduct_cbor::{Encoder, StreamEncoder, ToCbor, Write};

use crate::filter::{BundleFilter, BundleMetadata, BundleMutator, FilterChain};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Init,
    Blocks,
    Payload,
}

/// Streaming bundle encoder with optional egress filter pipeline.
///
/// Filters run before the first byte hits the wire.
/// Rejected bundles waste zero network I/O.
pub struct BundleWriter<W> {
    enc: StreamEncoder<W>,
    state: State,
    payload_hasher: Option<CrcHasher>,
    payload_crc: Crc,
    payload_remaining: u64,
    chain: Arc<FilterChain>,
    primary: Option<PrimaryBlock>,
    extensions: Vec<CanonicalBlock>,
    deferred: bool,
}

impl<W: Write> BundleWriter<W> {
    pub fn new(writer: W) -> Result<Self, Error> {
        Ok(Self {
            enc: StreamEncoder::new(writer),
            state: State::Init,
            payload_hasher: None,
            payload_crc: Crc::None,
            payload_remaining: 0,
            chain: Arc::new(FilterChain::new()),
            primary: None,
            extensions: Vec::new(),
            deferred: false,
        })
    }

    pub fn filter(mut self, f: impl BundleFilter + 'static) -> Self {
        Arc::get_mut(&mut self.chain)
            .expect("filter must be called before writing")
            .add_filter(f);
        self.deferred = true;
        self
    }

    pub fn mutator(mut self, m: impl BundleMutator + 'static) -> Self {
        Arc::get_mut(&mut self.chain)
            .expect("mutator must be called before writing")
            .add_mutator(m);
        self.deferred = true;
        self
    }

    pub fn write_primary(&mut self, primary: &PrimaryBlock) -> Result<(), Error> {
        if self.state != State::Init {
            return Err(Error::IncompleteRead);
        }
        if self.deferred {
            self.primary = Some(primary.clone());
        } else {
            self.enc.write_indefinite_array()?;
            let mut buf = Encoder::with_capacity(128);
            primary.encode(&mut buf);
            self.enc.write_raw(buf.as_bytes())?;
        }
        self.state = State::Blocks;
        Ok(())
    }

    pub fn write_extension(&mut self, block: &CanonicalBlock) -> Result<(), Error> {
        if self.state != State::Blocks {
            return Err(Error::IncompleteRead);
        }
        if self.deferred {
            self.extensions.push(block.clone());
        } else {
            let mut buf = Encoder::with_capacity(64);
            block.encode(&mut buf);
            self.enc.write_raw(buf.as_bytes())?;
        }
        Ok(())
    }

    pub fn begin_payload(
        &mut self,
        flags: BlockFlags,
        crc: Crc,
        data_len: u64,
    ) -> Result<(), Error> {
        if self.state != State::Blocks {
            return Err(Error::IncompleteRead);
        }

        // Egress filter gate: all metadata available
        if self.deferred {
            let mut primary = self.primary.take().ok_or(Error::IncompleteRead)?;
            let mut extensions = core::mem::take(&mut self.extensions);

            let meta = BundleMetadata {
                primary: &primary,
                extensions: &extensions,
                payload_len: data_len,
            };
            self.chain.run_filters(&meta)?;
            self.chain.run_mutators(&mut primary, &mut extensions);

            // Now write: array start + primary + extensions
            self.enc.write_indefinite_array()?;

            let mut buf = Encoder::with_capacity(128);
            primary.encode(&mut buf);
            self.enc.write_raw(buf.as_bytes())?;

            for ext in &extensions {
                let mut buf = Encoder::with_capacity(64);
                ext.encode(&mut buf);
                self.enc.write_raw(buf.as_bytes())?;
            }
        }

        let has_crc = !crc.is_none();
        let mut header = Encoder::with_capacity(16);
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
        if self.state != State::Payload {
            return Err(Error::PayloadNotConsumed);
        }
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
        if self.state != State::Payload {
            return Err(Error::PayloadNotConsumed);
        }
        if let Some(mut hasher) = self.payload_hasher.take() {
            let crc_size = self.payload_crc.value_size();
            let mut zeroed = [0u8; 5];
            zeroed[0] = 0x40 | crc_size as u8;
            hasher.update(&zeroed[..1 + crc_size]);

            let computed = hasher.finalize();
            let mut crc_buf = [0u8; 4];
            let n = computed.write_value(&mut crc_buf);
            self.enc.write_bstr(&crc_buf[..n])?;
        }
        self.state = State::Blocks;
        Ok(())
    }

    pub fn finish(self) -> Result<W, Error> {
        if self.state == State::Payload {
            return Err(Error::PayloadNotConsumed);
        }
        let mut enc = self.enc;
        enc.write_break()?;
        enc.flush()?;
        Ok(enc.into_inner())
    }
}
