pub mod builder;

use alloc::vec::Vec;
use core::ops::{Deref, DerefMut};

use bundle_bpv7::{Bundle as Bpv7Bundle, CanonicalBlock, Error, PrimaryBlock};
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

    pub fn retention(&self) -> &S {
        &self.retention
    }
}

impl<S: Retention> Bundle<S> {
    /// Stream the payload to a destination.
    ///
    /// Reads from retention in 64KB chunks and writes to `dest`.
    /// Returns the number of bytes written.
    pub fn payload(&self, dest: &mut impl Write) -> Result<u64, IoError> {
        let (offset, len) = self
            .payload_block()
            .and_then(|b| b.retained_range())
            .ok_or(IoError::UnexpectedEof)?;
        let mut reader = self.retention.reader(offset, len)?;
        let mut buf = [0u8; 65536];
        let mut written = 0u64;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            dest.write_all(&buf[..n])?;
            written += n as u64;
        }
        Ok(written)
    }

    pub fn encode_to<W: Write>(&self, writer: W) -> Result<(), Error> {
        crate::io::BundleWriter::new().write_to(self, writer)
    }
}

#[cfg(feature = "async")]
impl<S: AsyncRetention> Bundle<S> {
    /// Stream the payload to a destination (async).
    ///
    /// Reads from retention in 64KB chunks and writes to `dest`.
    /// Returns the number of bytes written.
    pub async fn async_payload(&self, dest: &mut impl Write) -> Result<u64, IoError> {
        let (offset, len) = self
            .payload_block()
            .and_then(|b| b.retained_range())
            .ok_or(IoError::UnexpectedEof)?;
        let mut reader = self.retention.reader(offset, len).await?;
        let mut buf = [0u8; 65536];
        let mut written = 0u64;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            dest.write_all(&buf[..n])?;
            written += n as u64;
        }
        Ok(written)
    }

    pub async fn async_encode_to<W: AsyncWrite + Unpin>(&self, writer: W) -> Result<(), Error> {
        self.validate()?;
        BundleAsyncWriter::new().write_to(self, writer).await
    }
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
