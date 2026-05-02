//! Async streaming bundle writer with optional egress filter pipeline.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use bundle_bpv7::{BlockFlags, CanonicalBlock, Crc, CrcHasher, Error, PrimaryBlock};
use bundle_cbor::{Encoder, ToCbor};
use bundle_io::Error as IoError;
use futures_io::AsyncWrite;

use crate::filter::{BundleFilter, BundleMetadata, BundleMutator, FilterChain};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Init,
    Blocks,
    Payload,
}

/// Async streaming bundle encoder with optional egress filter pipeline.
///
/// Filters run before the first byte hits the wire.
/// Rejected bundles waste zero network I/O.
pub struct BundleAsyncWriter<W> {
    writer: W,
    state: State,
    payload_hasher: Option<CrcHasher>,
    payload_crc: Crc,
    payload_remaining: u64,
    chain: Arc<FilterChain>,
    primary: Option<PrimaryBlock>,
    extensions: Vec<CanonicalBlock>,
    deferred: bool,
}

impl<W: AsyncWrite + Unpin> BundleAsyncWriter<W> {
    pub async fn new(writer: W) -> Result<Self, Error> {
        Ok(Self {
            writer,
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

    pub async fn write_primary(&mut self, primary: &PrimaryBlock) -> Result<(), Error> {
        if self.state != State::Init {
            return Err(Error::IncompleteRead);
        }
        if self.deferred {
            self.primary = Some(primary.clone());
        } else {
            write_all(&mut self.writer, &[0x9F]).await?;
            let mut buf = Encoder::with_capacity(128);
            primary.encode(&mut buf);
            write_all(&mut self.writer, buf.as_bytes()).await?;
        }
        self.state = State::Blocks;
        Ok(())
    }

    pub async fn write_extension(&mut self, block: &CanonicalBlock) -> Result<(), Error> {
        if self.state != State::Blocks {
            return Err(Error::IncompleteRead);
        }
        if self.deferred {
            self.extensions.push(block.clone());
        } else {
            let mut buf = Encoder::with_capacity(64);
            block.encode(&mut buf);
            write_all(&mut self.writer, buf.as_bytes()).await?;
        }
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

        // Egress filter gate
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

            write_all(&mut self.writer, &[0x9F]).await?;

            let mut buf = Encoder::with_capacity(128);
            primary.encode(&mut buf);
            write_all(&mut self.writer, buf.as_bytes()).await?;

            for ext in &extensions {
                let mut buf = Encoder::with_capacity(64);
                ext.encode(&mut buf);
                write_all(&mut self.writer, buf.as_bytes()).await?;
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
            let mut crc_buf = [0u8; 5];
            crc_buf[0] = 0x40 | crc_size as u8;
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
        write_all(&mut self.writer, &[0xFF]).await?;
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
        .map_err(|e| Error::Cbor(bundle_cbor::Error::Io(IoError::Io(e))))?;
        buf = &buf[n..];
    }
    Ok(())
}

async fn flush<W: AsyncWrite + Unpin>(writer: &mut W) -> Result<(), Error> {
    std::future::poll_fn(|cx| -> Poll<io::Result<()>> { Pin::new(&mut *writer).poll_flush(cx) })
        .await
        .map_err(|e| Error::Cbor(bundle_cbor::Error::Io(IoError::Io(e))))
}
