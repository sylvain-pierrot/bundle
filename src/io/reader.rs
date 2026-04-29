//! Streaming bundle reader — step-by-step state machine.

use std::io::Read;

use aqueduct_cbor::StreamDecoder;

use crate::bundle::Bundle;
use crate::bundle::canonical::{BlockFlags, CanonicalBlock};
use crate::bundle::crc::Crc;
use crate::bundle::payload::PayloadRef;
use crate::bundle::primary::PrimaryBlock;
use crate::error::Error;
use crate::io::retention::Retention;
use crate::io::tee::TeeReader;

use super::decode::{decode_canonical_body, decode_crc_value, decode_primary};

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
    Extension(CanonicalBlock),
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
    dec: StreamDecoder<TeeReader<R, S::Writer>>,
    retention: S,
    state: State,
    primary: Option<PrimaryBlock<'static>>,
    extensions: Vec<CanonicalBlock>,
    payload_flags: BlockFlags,
    payload_crc_type: u64,
    payload_crc: Crc,
    payload_data_offset: u64,
    payload_len: u64,
    payload_remaining: u64,
}

impl<R: Read, S: Retention> BundleReader<R, S> {
    pub fn new(source: R, retention: S) -> Result<Self, Error> {
        let writer = retention.writer().map_err(aqueduct_cbor::Error::from)?;
        let tee = TeeReader::new(source, writer);
        Ok(BundleReader {
            dec: StreamDecoder::new(tee),
            retention,
            state: State::Initial,
            primary: None,
            extensions: Vec::new(),
            payload_flags: BlockFlags::from_bits(0),
            payload_crc_type: 0,
            payload_crc: Crc::None,
            payload_data_offset: 0,
            payload_len: 0,
            payload_remaining: 0,
        })
    }

    pub fn next_block(&mut self) -> Result<Option<BlockEvent>, Error> {
        match self.state {
            State::Initial => {
                self.dec.read_indefinite_array_start()?;
                let primary = decode_primary(&mut self.dec)?;
                self.primary = Some(primary);
                self.state = State::Blocks;
            }
            State::Blocks => {}
            State::PayloadConsumed => {
                self.payload_crc = if self.payload_crc_type != 0 {
                    decode_crc_value(&mut self.dec, self.payload_crc_type)?
                } else {
                    Crc::None
                };
                self.state = State::Blocks;
            }
            State::PayloadData => return Err(Error::InvalidCbor),
            State::Done => return Ok(None),
        }

        if self.dec.is_break()? {
            self.dec.read_break()?;
            self.state = State::Done;
            return Ok(None);
        }

        let array_len = self.dec.read_array_len()?;
        let block_type = self.dec.read_uint()?;

        if block_type == 1 {
            if array_len != 5 && array_len != 6 {
                return Err(Error::InvalidCbor);
            }
            let _block_number = self.dec.read_uint()?;
            self.payload_flags = BlockFlags::from_bits(self.dec.read_uint()?);
            self.payload_crc_type = self.dec.read_uint()?;
            self.payload_len = self.dec.read_bstr_header()?;
            self.payload_data_offset = self.dec.position();
            self.payload_remaining = self.payload_len;
            self.state = State::PayloadData;
            Ok(Some(BlockEvent::Payload {
                len: self.payload_len,
            }))
        } else {
            let block = decode_canonical_body(&mut self.dec, block_type, array_len)?;
            self.extensions.push(block.clone());
            Ok(Some(BlockEvent::Extension(block)))
        }
    }

    pub fn primary(&self) -> Option<&PrimaryBlock<'static>> {
        self.primary.as_ref()
    }

    pub fn extensions(&self) -> &[CanonicalBlock] {
        &self.extensions
    }

    pub fn payload_len(&self) -> u64 {
        self.payload_len
    }

    pub fn payload_data_offset(&self) -> u64 {
        self.payload_data_offset
    }

    pub fn payload_flags(&self) -> BlockFlags {
        self.payload_flags
    }

    pub fn payload_crc(&self) -> Crc {
        self.payload_crc
    }

    pub fn payload_reader(&mut self) -> PayloadReader<'_, R, S> {
        PayloadReader { reader: self }
    }

    /// Walk `len` bytes forward in the stream.
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

    /// Parse all remaining blocks and assemble the [`Bundle`].
    pub fn into_bundle(mut self) -> Result<Bundle<S>, Error> {
        while let Some(event) = self.next_block()? {
            if let BlockEvent::Payload { len } = event {
                self.walk(len)?;
            }
        }

        if self.state != State::Done {
            return Err(Error::InvalidCbor);
        }

        let primary = self.primary.ok_or(Error::InvalidPayloadCount(0))?;

        Ok(Bundle::from_parts(
            primary,
            self.extensions,
            PayloadRef {
                flags: self.payload_flags,
                crc: self.payload_crc,
                data_offset: self.payload_data_offset,
                data_len: self.payload_len,
            },
            self.retention,
        ))
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
