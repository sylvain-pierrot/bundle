//! Async streaming bundle writer.

use std::io;
use std::pin::Pin;
use std::task::Poll;

use aqueduct_cbor::{Encoder, ToCbor};
use futures_io::AsyncWrite;

use crate::bundle::canonical::{BlockFlags, CanonicalBlock};
use crate::bundle::crc::{Crc, CrcHasher};
use crate::bundle::primary::PrimaryBlock;
use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Init,
    Blocks,
    Payload,
}

/// Async streaming bundle encoder.
///
/// Same logic as [`BundleWriter`](super::BundleWriter) but writes to
/// an [`AsyncWrite`] sink. Block headers are encoded synchronously
/// (CPU-bound, nanoseconds), then async-written to the sink.
pub struct BundleAsyncWriter<W> {
    writer: W,
    state: State,
    payload_hasher: Option<CrcHasher>,
    payload_crc: Crc,
    payload_remaining: u64,
}

impl<W: AsyncWrite + Unpin> BundleAsyncWriter<W> {
    pub async fn new(mut writer: W) -> Result<Self, Error> {
        write_all(&mut writer, &[0x9F]).await?; // indefinite array start
        Ok(Self {
            writer,
            state: State::Init,
            payload_hasher: None,
            payload_crc: Crc::None,
            payload_remaining: 0,
        })
    }

    pub async fn write_primary(&mut self, primary: &PrimaryBlock) -> Result<(), Error> {
        if self.state != State::Init {
            return Err(Error::IncompleteRead);
        }
        let mut buf = Encoder::with_capacity(128);
        primary.encode(&mut buf);
        write_all(&mut self.writer, buf.as_bytes()).await?;
        self.state = State::Blocks;
        Ok(())
    }

    pub async fn write_extension(&mut self, block: &CanonicalBlock) -> Result<(), Error> {
        if self.state != State::Blocks {
            return Err(Error::IncompleteRead);
        }
        let mut buf = Encoder::with_capacity(64);
        block.encode(&mut buf);
        write_all(&mut self.writer, buf.as_bytes()).await?;
        Ok(())
    }

    pub async fn begin_payload(
        &mut self,
        flags: BlockFlags,
        crc: Crc,
        data_len: u64,
    ) -> Result<(), Error> {
        if self.state != State::Blocks {
            return Err(Error::IncompleteRead);
        }
        let has_crc = !crc.is_none();

        let mut header = Encoder::with_capacity(16);
        header.write_array(if has_crc { 6 } else { 5 });
        header.write_uint(1);
        header.write_uint(1);
        header.write_uint(flags.bits());
        header.write_uint(crc.crc_type());
        header.write_bstr_header(data_len);

        write_all(&mut self.writer, header.as_bytes()).await?;

        self.payload_hasher = CrcHasher::new(&crc);
        if let Some(h) = &mut self.payload_hasher {
            h.update(header.as_bytes());
        }

        self.payload_crc = crc;
        self.payload_remaining = data_len;
        self.state = State::Payload;
        Ok(())
    }

    pub async fn write_payload_data(&mut self, data: &[u8]) -> Result<(), Error> {
        if self.state != State::Payload {
            return Err(Error::PayloadNotConsumed);
        }
        let len = data.len() as u64;
        if len > self.payload_remaining {
            return Err(Error::PayloadOverflow);
        }
        write_all(&mut self.writer, data).await?;
        if let Some(h) = &mut self.payload_hasher {
            h.update(data);
        }
        self.payload_remaining -= len;
        Ok(())
    }

    pub async fn end_payload(&mut self) -> Result<(), Error> {
        if self.state != State::Payload {
            return Err(Error::PayloadNotConsumed);
        }
        if let Some(mut hasher) = self.payload_hasher.take() {
            let crc_size = self.payload_crc.value_size();
            let mut zeroed = [0u8; 5];
            zeroed[0] = 0x40 | crc_size as u8;
            hasher.update(&zeroed[..1 + crc_size]);

            let computed = hasher.finalize();
            let mut crc_buf = [0u8; 5]; // 1 byte header + max 4 bytes value
            crc_buf[0] = 0x40 | crc_size as u8; // CBOR bstr header
            let n = computed.write_value(&mut crc_buf[1..]);
            write_all(&mut self.writer, &crc_buf[..1 + n]).await?;
        }
        self.state = State::Blocks;
        Ok(())
    }

    pub async fn finish(mut self) -> Result<W, Error> {
        if self.state == State::Payload {
            return Err(Error::PayloadNotConsumed);
        }
        write_all(&mut self.writer, &[0xFF]).await?; // break
        flush(&mut self.writer).await?;
        Ok(self.writer)
    }
}

async fn write_all<W: AsyncWrite + Unpin>(writer: &mut W, mut buf: &[u8]) -> Result<(), Error> {
    while !buf.is_empty() {
        let n = std::future::poll_fn(|cx| -> Poll<io::Result<usize>> {
            Pin::new(&mut *writer).poll_write(cx, buf)
        })
        .await
        .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))?;
        buf = &buf[n..];
    }
    Ok(())
}

async fn flush<W: AsyncWrite + Unpin>(writer: &mut W) -> Result<(), Error> {
    std::future::poll_fn(|cx| -> Poll<io::Result<()>> { Pin::new(&mut *writer).poll_flush(cx) })
        .await
        .map_err(|e| Error::Cbor(aqueduct_cbor::Error::from(e)))
}
