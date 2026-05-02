//! Step-by-step bundle parsing session.

use alloc::sync::Arc;
use alloc::vec::Vec;

use bundle_bpv7::{CanonicalBlock, Crc, Error, PrimaryBlock};
use bundle_cbor::{Encoder, StreamDecoder, ToCbor};
use bundle_io::Read;

use super::{BlockEvent, ReadResult};
use crate::bundle::Bundle;
use crate::filter::{BundleMetadata, FilterChain};
use crate::io::adapters::{CaptureReader, DeferredReader, TeeReader};
use crate::retention::Retention;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Initial,
    Blocks,
    PayloadData,
    PayloadConsumed,
    Done,
}

/// An active parsing session, created by [`BundleReader::open`](super::BundleReader::open).
pub struct OpenBundleReader<R, S: Retention> {
    pub(crate) dec: StreamDecoder<DeferredReader<R, S>>,
    pub(crate) state: State,
    pub(crate) primary: Option<PrimaryBlock>,
    pub(crate) blocks: Vec<CanonicalBlock>,
    pub(crate) payload_idx: Option<usize>,
    pub(crate) payload_crc_type: u64,
    pub(crate) payload_remaining: u64,
    pub(crate) retention: Option<S>,
    pub(crate) chain: Arc<FilterChain>,
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
                        let mut enc = Encoder::with_capacity(256);
                        enc.write_indefinite_array();
                        self.primary
                            .as_ref()
                            .ok_or(Error::IncompleteRead)?
                            .encode(&mut enc);
                        for ext in &self.blocks {
                            ext.encode(&mut enc);
                        }
                        let has_crc = block.crc.crc_type() != 0;
                        enc.write_array(if has_crc { 6 } else { 5 });
                        enc.write_uint(block.block_type);
                        enc.write_uint(block.block_number);
                        enc.write_uint(block.flags.bits());
                        enc.write_uint(block.crc.crc_type());
                        enc.write_bstr_header(data_len);

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

    pub fn into_bundle(mut self) -> Result<ReadResult<S>, Error> {
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
                    return Ok(ReadResult::Accepted(Bundle::from_parts(
                        primary,
                        self.blocks,
                        retention,
                    )));
                }
                Err(Error::FilterRejected(r)) => {
                    if let Some(ref mut ret) = self.retention {
                        let _ = ret.discard();
                    }
                    return Ok(ReadResult::Rejected(r));
                }
                Err(e) => break e,
            }
        };

        let mut retention = self.dec.into_inner().into_retention().or(self.retention);
        if let Some(ref mut r) = retention {
            let _ = r.discard();
        }
        Err(err)
    }
}
