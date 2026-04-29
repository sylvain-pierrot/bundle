//! Streaming bundle reader.

use std::io::Read;

use aqueduct_cbor::StreamDecoder;

use crate::bundle::Bundle;
use crate::bundle::canonical::CanonicalBlock;
use crate::bundle::crc::Crc;
use crate::bundle::primary::PrimaryBlock;
use crate::error::Error;
use crate::io::tee::TeeReader;
use crate::retention::Retention;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Initial,
    Blocks,
    PayloadData,
    PayloadConsumed,
    Done,
}

/// What [`BundleReader::next_block`] yielded.
pub enum BlockEvent {
    Extension(usize),
    Payload { len: u64 },
}

/// Stateless streaming bundle parser.
///
/// ```text
/// let reader = BundleReader::new();
/// let bundle = reader.read_from(socket, retention)?;
/// ```
///
/// Or step-by-step via [`open`](Self::open) + [`next_block`](Self::next_block):
///
/// ```text
///  open(source, retention)
///    │
///    ▼
///  ┌─────────────────────┐
///  │  next_block()       │◄──────────────────────┐
///  └──┬──────────────┬───┘                       │
///     │              │                           │
///  Extension      Payload { len }             (loop)
///     │              │                           │
///     │         payload_reader()                 │
///     │             or                           │
///     │         walk(len)                        │
///     │              │                           │
///     └──────────────┴───────────────────────────┘
///     │
///  None (end of bundle)
///     │
///     ▼
///  into_bundle() → Bundle<S>
/// ```
pub struct BundleReader;

impl BundleReader {
    pub fn new() -> Self {
        BundleReader
    }

    /// Parse a bundle from a source, storing wire bytes in the retention.
    /// One-shot: reads, parses, and returns the bundle.
    pub fn read_from<R: Read, S: Retention>(
        &self,
        source: R,
        retention: S,
    ) -> Result<Bundle<S>, Error> {
        OpenBundleReader::open(source, retention).into_bundle()
    }

    /// Open a source for step-by-step parsing.
    pub fn open<R: Read, S: Retention>(&self, source: R, retention: S) -> OpenBundleReader<R, S> {
        OpenBundleReader::open(source, retention)
    }
}

impl Default for BundleReader {
    fn default() -> Self {
        Self::new()
    }
}

/// An active parsing session, created by [`BundleReader::open`].
pub struct OpenBundleReader<R, S: Retention> {
    dec: StreamDecoder<TeeReader<R, S>>,
    state: State,
    primary: Option<PrimaryBlock>,
    blocks: Vec<CanonicalBlock>,
    payload_idx: Option<usize>,
    payload_crc_type: u64,
    payload_remaining: u64,
}

impl<R: Read, S: Retention> OpenBundleReader<R, S> {
    pub(crate) fn open(source: R, retention: S) -> Self {
        let tee = TeeReader::new(source, retention);
        OpenBundleReader {
            dec: StreamDecoder::new(tee),
            state: State::Initial,
            primary: None,
            blocks: Vec::new(),
            payload_idx: None,
            payload_crc_type: 0,
            payload_remaining: 0,
        }
    }

    pub fn next_block(&mut self) -> Result<Option<BlockEvent>, Error> {
        match self.state {
            State::Initial => {
                self.dec.read_indefinite_array_start()?;
                let primary = PrimaryBlock::decode(&mut self.dec)?;
                primary.verify_crc()?;
                self.primary = Some(primary);
                self.state = State::Blocks;
            }
            State::Blocks => {}
            State::PayloadConsumed => {
                let idx = self.payload_idx.unwrap();
                self.blocks[idx].crc = if self.payload_crc_type != 0 {
                    Crc::decode(&mut self.dec, self.payload_crc_type)?
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

        let (block, has_data_in_stream) = CanonicalBlock::decode(&mut self.dec)?;

        if has_data_in_stream {
            if self.payload_idx.is_some() {
                return Err(Error::InvalidPayloadCount(2));
            }
            let data_len = block.retained_range().ok_or(Error::PayloadNotInline)?.1;
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

    pub fn payload_reader(&mut self) -> PayloadReader<'_, R, S> {
        PayloadReader { reader: self }
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
                    let (_source, mut retention) = self.dec.into_inner().into_parts();
                    if let Err(e) = retention.flush() {
                        let _ = retention.discard();
                        return Err(Error::Cbor(aqueduct_cbor::Error::from(e)));
                    }
                    return Ok(Bundle::from_parts(primary, self.blocks, retention));
                }
                Err(e) => break e,
            }
        };

        let (_source, mut retention) = self.dec.into_inner().into_parts();
        let _ = retention.discard();
        Err(err)
    }
}

/// A reader that yields exactly the payload bytes.
pub struct PayloadReader<'a, R, S: Retention> {
    reader: &'a mut OpenBundleReader<R, S>,
}

impl<R: Read, S: Retention> Read for PayloadReader<'_, R, S> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
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
}
