//! Storage backends for bundle retention.

mod disk;
mod memory;

use std::io::{self, Read, Write};

pub use disk::DiskRetention;
pub use memory::MemoryRetention;

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

    /// Discard all stored bytes. Called when parsing fails.
    fn discard(&mut self) -> io::Result<()>;
}

/// Async storage backend for bundle retention.
#[cfg(feature = "async")]
pub trait AsyncRetention: futures_io::AsyncWrite + Unpin {
    type Reader<'a>: futures_io::AsyncRead + Unpin
    where
        Self: 'a;

    fn reader(&self, offset: u64, len: u64) -> io::Result<Self::Reader<'_>>;
    fn discard(&mut self) -> io::Result<()>;
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
