//! Storage backends for bundle retention.

#[cfg(feature = "std")]
mod disk;
#[cfg(feature = "embedded-storage")]
mod flash;
mod memory;
#[cfg(feature = "async")]
pub mod s3;

use bundle_io::{Error as IoError, Read, Write};

pub use memory::MemoryRetention;

#[cfg(feature = "std")]
pub use disk::DiskRetention;
#[cfg(feature = "embedded-storage")]
pub use flash::FlashRetention;

#[cfg(feature = "async")]
use async_trait::async_trait;
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

    fn reader(&self, offset: u64, len: u64) -> Result<Self::Reader<'_>, IoError>;
    fn discard(&mut self) -> Result<(), IoError>;
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

    async fn write(&mut self, data: &[u8]) -> Result<usize, IoError>;

    async fn write_all(&mut self, mut data: &[u8]) -> Result<(), IoError> {
        while !data.is_empty() {
            let n = self.write(data).await?;
            if n == 0 {
                return Err(IoError::UnexpectedEof);
            }
            data = &data[n..];
        }
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), IoError>;
    async fn reader(&self, offset: u64, len: u64) -> Result<Self::Reader<'_>, IoError>;
    async fn discard(&mut self) -> Result<(), IoError>;
}

/// No-op retention that discards writes.
#[cfg(feature = "async")]
pub(crate) struct NoopRetention;

#[cfg(feature = "async")]
impl Write for NoopRetention {
    fn write_all(&mut self, _buf: &[u8]) -> Result<(), IoError> {
        Ok(())
    }

    fn flush(&mut self) -> Result<(), IoError> {
        Ok(())
    }
}

#[cfg(feature = "async")]
impl Retention for NoopRetention {
    type Reader<'a> = &'a [u8];

    fn reader(&self, _offset: u64, _len: u64) -> Result<Self::Reader<'_>, IoError> {
        Ok(&[])
    }

    fn discard(&mut self) -> Result<(), IoError> {
        Ok(())
    }
}
