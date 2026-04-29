//! Streaming bundle reader — step-by-step state machine.

use std::io::Read;

use aqueduct_cbor::StreamDecoder;

use crate::bundle::Bundle;
use crate::bundle::canonical::{BlockData, CanonicalBlock};
use crate::bundle::crc::Crc;
use crate::bundle::primary::PrimaryBlock;
use crate::error::Error;
use crate::io::retention::Retention;
use crate::io::tee::TeeReader;

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
    /// An extension block was parsed (index into [`BundleReader::blocks`]).
    Extension(usize),
    /// The payload block header was parsed. Data follows in the stream.
    Payload { len: u64 },
}

/// Streaming bundle parser.
///
/// ```text
///  new(source, retention)
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
pub struct BundleReader<R, S: Retention> {
    dec: StreamDecoder<TeeReader<R, S>>,
    state: State,
    primary: Option<PrimaryBlock>,
    blocks: Vec<CanonicalBlock>,
    payload_idx: Option<usize>,
    payload_crc_type: u64,
    payload_remaining: u64,
}

impl<R: Read, S: Retention> BundleReader<R, S> {
    pub fn new(source: R, retention: S) -> Self {
        let tee = TeeReader::new(source, retention);
        BundleReader {
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
                self.primary = Some(primary);
                self.state = State::Blocks;
            }
            State::Blocks => {}
            State::PayloadConsumed => {
                // Read the CRC that follows the payload data
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
            // Payload block — data follows in the stream
            if self.payload_idx.is_some() {
                return Err(Error::InvalidPayloadCount(2));
            }
            let data_len = match &block.data {
                BlockData::Retained { len, .. } => *len,
                _ => unreachable!(),
            };
            self.payload_crc_type = block.crc.crc_type();
            self.payload_remaining = data_len;
            self.payload_idx = Some(self.blocks.len());
            self.blocks.push(block);
            self.state = State::PayloadData;
            Ok(Some(BlockEvent::Payload { len: data_len }))
        } else {
            // Extension block — fully parsed
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
        while let Some(event) = self.next_block()? {
            if let BlockEvent::Payload { len } = event {
                self.walk(len)?;
            }
        }

        if self.state != State::Done {
            return Err(Error::IncompleteRead);
        }

        let primary = self.primary.ok_or(Error::InvalidPayloadCount(0))?;

        let (_source, mut retention) = self.dec.into_inner().into_parts();
        retention.flush().map_err(aqueduct_cbor::Error::from)?;

        Ok(Bundle::from_parts(primary, self.blocks, retention))
    }
}

/// A reader that yields exactly the payload bytes from a [`BundleReader`].
pub struct PayloadReader<'a, R, S: Retention> {
    reader: &'a mut BundleReader<R, S>,
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
