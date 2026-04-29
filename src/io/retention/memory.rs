use std::io::Write;

use super::Retention;

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
