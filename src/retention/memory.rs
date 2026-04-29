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

    fn reader(&self, offset: u64, len: u64) -> std::io::Result<Self::Reader<'_>> {
        let start = usize::try_from(offset).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "offset overflows usize")
        })?;
        let end = start
            .checked_add(usize::try_from(len).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "len overflows usize")
            })?)
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "offset + len overflow")
            })?;
        if end > self.data.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "retention read out of bounds: offset={start}, len={len}, stored={}",
                    self.data.len()
                ),
            ));
        }
        Ok(&self.data[start..end])
    }

    fn discard(&mut self) -> std::io::Result<()> {
        self.data.clear();
        Ok(())
    }
}
