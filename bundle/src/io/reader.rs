//! Streaming bundle reader with optional ingress filter pipeline.

use alloc::sync::Arc;
use alloc::vec::Vec;

use bundle_bpv7::{CanonicalBlock, Crc, Error, PrimaryBlock};
use bundle_cbor::{Encoder, StreamDecoder, ToCbor};
use bundle_io::{Error as IoError, Read};

use crate::bundle::Bundle;
use crate::filter::{BundleFilter, BundleMetadata, BundleMutator, FilterChain};
use crate::io::adapters::{CaptureReader, DeferredReader, TeeReader};
use crate::retention::Retention;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Initial,
    Blocks,
    PayloadData,
    PayloadConsumed,
    Done,
}

/// What [`OpenBundleReader::next_block`] yielded.
pub enum BlockEvent {
    Extension(usize),
    Payload { len: u64 },
}

/// Streaming bundle parser with optional ingress filter pipeline.
///
/// Filters run in-flight before the payload touches retention.
/// Rejected bundles waste zero storage I/O.
pub struct BundleReader {
    chain: Arc<FilterChain>,
}

impl BundleReader {
    pub fn new() -> Self {
        BundleReader {
            chain: Arc::new(FilterChain::new()),
        }
    }

    pub fn filter(mut self, f: impl BundleFilter + 'static) -> Self {
        Arc::get_mut(&mut self.chain)
            .expect("filter must be called before cloning the reader")
            .add_filter(f);
        self
    }

    pub fn mutator(mut self, m: impl BundleMutator + 'static) -> Self {
        Arc::get_mut(&mut self.chain)
            .expect("mutator must be called before cloning the reader")
            .add_mutator(m);
        self
    }

    /// Parse a bundle from a source, storing wire bytes in the retention.
    pub fn read_from<R: Read, S: Retention>(
        &self,
        source: R,
        retention: S,
    ) -> Result<Bundle<S>, Error> {
        OpenBundleReader::open(source, retention, self.chain.clone()).into_bundle()
    }

    /// Open a source for step-by-step parsing.
    pub fn open<R: Read, S: Retention>(&self, source: R, retention: S) -> OpenBundleReader<R, S> {
        OpenBundleReader::open(source, retention, self.chain.clone())
    }
}

impl Default for BundleReader {
    fn default() -> Self {
        Self::new()
    }
}

/// An active parsing session, created by [`BundleReader::open`].
pub struct OpenBundleReader<R, S: Retention> {
    dec: StreamDecoder<DeferredReader<R, S>>,
    state: State,
    primary: Option<PrimaryBlock>,
    blocks: Vec<CanonicalBlock>,
    payload_idx: Option<usize>,
    payload_crc_type: u64,
    payload_remaining: u64,
    retention: Option<S>,
    chain: Arc<FilterChain>,
}

impl<R: Read, S: Retention> OpenBundleReader<R, S> {
    pub(crate) fn open(source: R, retention: S, chain: Arc<FilterChain>) -> Self {
        if chain.is_empty() {
            let tee = TeeReader::new(source, retention);
            OpenBundleReader {
                dec: StreamDecoder::new(DeferredReader::Teeing(tee)),
                state: State::Initial,
                primary: None,
                blocks: Vec::new(),
                payload_idx: None,
                payload_crc_type: 0,
                payload_remaining: 0,
                retention: None,
                chain,
            }
        } else {
            let capture = CaptureReader::new(source);
            OpenBundleReader {
                dec: StreamDecoder::new(DeferredReader::Capturing(capture)),
                state: State::Initial,
                primary: None,
                blocks: Vec::new(),
                payload_idx: None,
                payload_crc_type: 0,
                payload_remaining: 0,
                retention: Some(retention),
                chain,
            }
        }
    }

    pub fn next_block(&mut self) -> Result<Option<BlockEvent>, Error> {
        match self.state {
            State::Initial => {
                self.dec.read_indefinite_array_start()?;
                let primary = PrimaryBlock::decode_stream(&mut self.dec)?;
                primary.verify_crc()?;
                self.primary = Some(primary);
                self.state = State::Blocks;
            }
            State::Blocks => {}
            State::PayloadConsumed => {
                let idx = self.payload_idx.ok_or(Error::InvalidPayloadCount(0))?;
                self.blocks[idx].crc = if self.payload_crc_type != 0 {
                    Crc::decode_stream(&mut self.dec, self.payload_crc_type)?
                } else {
                    Crc::None
                };
                self.state = State::Blocks;
            }
            State::PayloadData => return Err(Error::PayloadNotConsumed),
            State::Done => return Ok(None),
        }

        if self.dec.is_break()? {
            self.dec.read_break()?;
            self.state = State::Done;
            return Ok(None);
        }

        let (mut block, has_data_in_stream) = CanonicalBlock::decode_stream(&mut self.dec)?;

        if has_data_in_stream {
            if self.payload_idx.is_some() {
                return Err(Error::InvalidPayloadCount(2));
            }
            let data_len = block.retained_range().ok_or(Error::PayloadNotInline)?.1;

            if !self.chain.is_empty() {
                let meta = BundleMetadata {
                    primary: self.primary.as_ref().ok_or(Error::IncompleteRead)?,
                    extensions: &self.blocks,
                    payload_len: data_len,
                };
                self.chain.run_filters(&meta)?;

                let mutated = self.chain.run_mutators(
                    self.primary.as_mut().ok_or(Error::IncompleteRead)?,
                    &mut self.blocks,
                );

                if let Some(retention) = self.retention.take() {
                    if mutated {
                        // Re-encode mutated headers so storage holds the mutated version.
                        let mut enc = Encoder::with_capacity(256);
                        enc.write_indefinite_array();
                        self.primary
                            .as_ref()
                            .ok_or(Error::IncompleteRead)?
                            .encode(&mut enc);
                        for ext in &self.blocks {
                            ext.encode(&mut enc);
                        }
                        // Payload block header (data streams through after this).
                        let has_crc = block.crc.crc_type() != 0;
                        enc.write_array(if has_crc { 6 } else { 5 });
                        enc.write_uint(block.block_type);
                        enc.write_uint(block.block_number);
                        enc.write_uint(block.flags.bits());
                        enc.write_uint(block.crc.crc_type());
                        enc.write_bstr_header(data_len);

                        // Update payload offset to match the re-encoded layout.
                        let new_offset = enc.position() as u64;
                        block.data = bundle_bpv7::BlockData::Retained {
                            offset: new_offset,
                            len: data_len,
                        };

                        self.dec
                            .inner()
                            .activate_retention_replacing(retention, enc.as_bytes())?;
                    } else {
                        self.dec.inner().activate_retention(retention)?;
                    }
                }
            }

            self.payload_crc_type = block.crc.crc_type();
            self.payload_remaining = data_len;
            self.payload_idx = Some(self.blocks.len());
            self.blocks.push(block);
            self.state = State::PayloadData;
            Ok(Some(BlockEvent::Payload { len: data_len }))
        } else {
            block.verify_crc()?;
            self.blocks.push(block);
            Ok(Some(BlockEvent::Extension(self.blocks.len() - 1)))
        }
    }

    pub fn primary(&self) -> Option<&PrimaryBlock> {
        self.primary.as_ref()
    }

    pub fn blocks(&self) -> &[CanonicalBlock] {
        &self.blocks
    }

    pub fn payload_reader(&mut self) -> Result<PayloadReader<'_, R, S>, Error> {
        if self.state != State::PayloadData {
            return Err(Error::PayloadNotConsumed);
        }
        Ok(PayloadReader { reader: self })
    }

    pub fn walk(&mut self, len: u64) -> Result<(), Error> {
        self.dec.skip(len)?;
        if self.state == State::PayloadData {
            self.payload_remaining = self.payload_remaining.saturating_sub(len);
            if self.payload_remaining == 0 {
                self.state = State::PayloadConsumed;
            }
        }
        Ok(())
    }

    pub fn into_bundle(mut self) -> Result<Bundle<S>, Error> {
        let err = loop {
            match self.next_block() {
                Ok(Some(BlockEvent::Payload { len })) => {
                    if let Err(e) = self.walk(len) {
                        break e;
                    }
                }
                Ok(Some(BlockEvent::Extension(_))) => {}
                Ok(None) => {
                    if self.state != State::Done {
                        break Error::IncompleteRead;
                    }
                    let primary = match self.primary {
                        Some(p) => p,
                        None => break Error::InvalidPayloadCount(0),
                    };
                    let mut retention = self
                        .dec
                        .into_inner()
                        .into_retention()
                        .or(self.retention)
                        .ok_or(Error::IncompleteRead)?;
                    if let Err(e) = retention.flush() {
                        let _ = retention.discard();
                        return Err(Error::Cbor(bundle_cbor::Error::Io(e)));
                    }
                    return Ok(Bundle::from_parts(primary, self.blocks, retention));
                }
                Err(e) => break e,
            }
        };

        // Error path: discard retention
        let mut retention = self.dec.into_inner().into_retention().or(self.retention);
        if let Some(ref mut r) = retention {
            let _ = r.discard();
        }
        Err(err)
    }
}

/// A reader that yields exactly the payload bytes.
pub struct PayloadReader<'a, R, S: Retention> {
    reader: &'a mut OpenBundleReader<R, S>,
}

impl<R: Read, S: Retention> Read for PayloadReader<'_, R, S> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        if self.reader.payload_remaining == 0 {
            return Ok(0);
        }
        let max = buf.len().min(self.reader.payload_remaining as usize);
        let n = self.reader.dec.inner().read(&mut buf[..max])?;
        self.reader.payload_remaining -= n as u64;
        self.reader.dec.advance(n as u64);
        if self.reader.payload_remaining == 0 {
            self.reader.state = State::PayloadConsumed;
        }
        Ok(n)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), IoError> {
        let mut offset = 0;
        while offset < buf.len() {
            let n = self.read(&mut buf[offset..])?;
            if n == 0 {
                return Err(IoError::UnexpectedEof);
            }
            offset += n;
        }
        Ok(())
    }
}
