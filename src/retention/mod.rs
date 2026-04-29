//! Storage backends for bundle retention.

mod disk;
mod memory;
#[cfg(feature = "async")]
pub mod s3;

use std::io::{self, Read, Write};

#[cfg(feature = "async")]
use async_trait::async_trait;

pub use disk::DiskRetention;
pub use memory::MemoryRetention;

#[cfg(feature = "async")]
pub use s3::{S3Ops, S3Retention};

/// Storage backend where bundle bytes are retained.
///
/// Bytes are written during reception via [`Write`]. If parsing
/// succeeds, the retention holds a valid bundle. If parsing fails,
/// call [`discard`](Self::discard) to roll back.
pub trait Retention: Write {
    type Reader<'a>: Read
    where
        Self: 'a;

    fn reader(&self, offset: u64, len: u64) -> io::Result<Self::Reader<'_>>;
    fn discard(&mut self) -> io::Result<()>;
}

/// Async retention backend.
///
/// Async counterpart of [`Retention`]. Used by
/// [`BundleAsyncReader`](crate::BundleAsyncReader) and
/// [`BundleBuilder::from_async_stream`](crate::BundleBuilder::from_async_stream).
#[cfg(feature = "async")]
#[async_trait]
pub trait AsyncRetention: Send {
    type Reader<'a>: Read
    where
        Self: 'a;

    async fn write(&mut self, data: &[u8]) -> io::Result<usize>;

    async fn write_all(&mut self, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            let n = self.write(data).await?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "write returned 0"));
            }
            data = &data[n..];
        }
        Ok(())
    }

    async fn flush(&mut self) -> io::Result<()>;
    async fn reader(&self, offset: u64, len: u64) -> io::Result<Self::Reader<'_>>;
    async fn discard(&mut self) -> io::Result<()>;
}

/// No-op retention that discards writes.
pub(crate) struct NoopRetention;

impl Write for NoopRetention {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Retention for NoopRetention {
    type Reader<'a> = &'a [u8];

    fn reader(&self, _offset: u64, _len: u64) -> io::Result<Self::Reader<'_>> {
        Ok(&[])
    }

    fn discard(&mut self) -> io::Result<()> {
        Ok(())
    }
}
