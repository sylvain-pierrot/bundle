pub mod builder;

use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};

use bundle_bpv7::{BlockData, Bundle as Bpv7Bundle, CanonicalBlock, Error, PrimaryBlock};
use bundle_cbor::{Encoder, ToCbor};
use bundle_io::{Error as IoError, Read, Write};

use crate::retention::Retention;

#[cfg(feature = "async")]
use crate::io::BundleAsyncWriter;
#[cfg(feature = "async")]
use crate::retention::AsyncRetention;
#[cfg(feature = "async")]
use futures_io::AsyncWrite;

/// A BPv7 bundle backed by a retention storage.
///
/// Wraps [`Bpv7Bundle`] with a retention backend for
/// payload access. All metadata methods are available via `Deref`.
#[derive(Debug, Clone)]
pub struct Bundle<S> {
    inner: Bpv7Bundle,
    retention: S,
}

impl<S> Deref for Bundle<S> {
    type Target = Bpv7Bundle;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<S> DerefMut for Bundle<S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<S> Bundle<S> {
    pub(crate) fn from_parts(
        primary: PrimaryBlock,
        blocks: Vec<CanonicalBlock>,
        retention: S,
    ) -> Self {
        Bundle {
            inner: Bpv7Bundle::from_parts(primary, blocks),
            retention,
        }
    }

    #[cfg(feature = "async")]
    pub(crate) fn swap_retention<T>(self, retention: T) -> Bundle<T> {
        Bundle {
            inner: self.inner,
            retention,
        }
    }

    pub fn retention(&self) -> &S {
        &self.retention
    }
}

impl<S: Retention> Bundle<S> {
    pub fn payload_reader(&self) -> Result<S::Reader<'_>, IoError> {
        let (offset, len) = self
            .payload_block()
            .and_then(|b| b.retained_range())
            .ok_or(IoError::UnexpectedEof)?;
        self.retention.reader(offset, len)
    }

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        self.validate()?;

        let mut enc = Encoder::with_capacity(256);
        enc.write_indefinite_array();
        self.primary().encode(&mut enc);

        for block in self.blocks() {
            match &block.data {
                BlockData::Inline(_) => block.encode(&mut enc),
                BlockData::Retained { offset, len } => {
                    let mut payload_data = alloc::vec![0u8; *len as usize];
                    self.retention
                        .reader(*offset, *len)?
                        .read_exact(&mut payload_data)?;

                    let has_crc = !block.crc.is_none();
                    let block_start = enc.position();
                    enc.write_array(if has_crc { 6 } else { 5 });
                    enc.write_uint(block.block_type);
                    enc.write_uint(block.block_number);
                    enc.write_uint(block.flags.bits());
                    enc.write_uint(block.crc.crc_type());
                    enc.write_bstr(&payload_data);
                    block.crc.encode_and_finalize(&mut enc, block_start);
                }
            }
        }

        enc.write_break();
        Ok(enc.into_bytes())
    }

    pub fn encode_to<W: Write>(&self, writer: W) -> Result<(), Error> {
        crate::io::BundleWriter::new().write_to(self, writer)
    }
}

#[cfg(feature = "async")]
impl<S: AsyncRetention> Bundle<S> {
    pub async fn async_payload_reader(&self) -> Result<S::Reader<'_>, IoError> {
        let (offset, len) = self
            .payload_block()
            .and_then(|b| b.retained_range())
            .ok_or(IoError::UnexpectedEof)?;
        self.retention.reader(offset, len).await
    }

    pub async fn async_encode_to<W: AsyncWrite + Unpin>(&self, writer: W) -> Result<(), Error> {
        self.validate()?;
        let mut w = BundleAsyncWriter::new(writer).await?;
        w.write_primary(self.primary()).await?;

        for block in self.blocks() {
            match &block.data {
                BlockData::Inline(_) => w.write_extension(block).await?,
                BlockData::Retained { offset, len } => {
                    w.begin_payload(block.flags, block.crc, *len).await?;
                    let mut reader = self.retention.reader(*offset, *len).await?;
                    let mut buf = [0u8; 65536];
                    loop {
                        let n = reader.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        w.write_payload_data(&buf[..n]).await?;
                    }
                    w.end_payload().await?;
                }
            }
        }

        w.finish().await?;
        Ok(())
    }
}
