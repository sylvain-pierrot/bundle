//! Storage backend trait for bundle retention.

use std::io::{Read, Write};

/// Storage backend where bundle bytes are retained.
///
/// The retention itself is the writer — bytes are written directly
/// via the [`Write`] impl. Later, byte ranges can be read back via
/// [`reader`](Self::reader).
pub trait Retention: Write {
    type Reader<'a>: Read
    where
        Self: 'a;

    fn reader(&self, offset: u64, len: u64) -> Self::Reader<'_>;
}

/// In-memory retention backed by a `Vec<u8>`.
#[derive(Default, Debug, Clone)]
pub struct MemoryRetention {
    data: Vec<u8>,
}

impl MemoryRetention {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}

impl Write for MemoryRetention {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.data.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Retention for MemoryRetention {
    type Reader<'a> = &'a [u8];

    fn reader(&self, offset: u64, len: u64) -> Self::Reader<'_> {
        let start = offset as usize;
        let end = start + len as usize;
        &self.data[start..end]
    }
}
