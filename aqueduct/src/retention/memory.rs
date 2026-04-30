use aqueduct_io::{Error as IoError, Write};

use super::Retention;

/// In-memory retention backed by a `Vec<u8>`.
#[derive(Default, Debug, Clone)]
pub struct MemoryRetention {
    data: alloc::vec::Vec<u8>,
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
    fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        self.data.extend_from_slice(buf);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), IoError> {
        Ok(())
    }
}

impl Retention for MemoryRetention {
    type Reader<'a> = &'a [u8];

    fn reader(&self, offset: u64, len: u64) -> Result<Self::Reader<'_>, IoError> {
        let start = usize::try_from(offset).map_err(|_| IoError::UnexpectedEof)?;
        let end = start
            .checked_add(usize::try_from(len).map_err(|_| IoError::UnexpectedEof)?)
            .ok_or(IoError::UnexpectedEof)?;
        if end > self.data.len() {
            return Err(IoError::UnexpectedEof);
        }
        Ok(&self.data[start..end])
    }

    fn discard(&mut self) -> Result<(), IoError> {
        self.data.clear();
        Ok(())
    }
}
