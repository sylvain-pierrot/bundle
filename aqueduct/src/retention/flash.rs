//! Flash-backed retention for no_std persistent storage.
//!
//! Wraps any [`NorFlash`] implementation (SPI flash, QSPI, internal
//! flash, FRAM) to persist bundle data across power cycles.
//!
//! Writes are buffered to satisfy flash write-size alignment.

use alloc::vec::Vec;
use core::cell::RefCell;

use aqueduct_io::{Error as IoError, Read, Write};
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};

use super::Retention;

/// Flash-backed retention using `embedded-storage` traits.
///
/// Writes bundle data sequentially starting at `base_offset`.
/// Reads back via `ReadNorFlash`. Erase on discard.
///
/// Writes are buffered internally to satisfy flash write-size
/// alignment. Call `flush()` to write any remaining buffered data.
pub struct FlashRetention<F: NorFlash> {
    flash: RefCell<F>,
    base_offset: u32,
    write_offset: u32,
    erase_size: u32,
    write_size: u32,
    write_buf: Vec<u8>,
}

impl<F: NorFlash> FlashRetention<F> {
    pub fn new(flash: F, base_offset: u32) -> Self {
        let erase_size = F::ERASE_SIZE as u32;
        let write_size = F::WRITE_SIZE as u32;
        Self {
            flash: RefCell::new(flash),
            base_offset,
            write_offset: 0,
            erase_size,
            write_size,
            write_buf: Vec::with_capacity(write_size as usize),
        }
    }

    fn flush_buf(&mut self) -> Result<(), IoError> {
        if self.write_buf.is_empty() {
            return Ok(());
        }
        // Pad to write-size alignment
        let ws = self.write_size as usize;
        while !self.write_buf.len().is_multiple_of(ws) {
            self.write_buf.push(0xFF); // flash erased state
        }
        let offset = self.base_offset + self.write_offset;
        self.flash
            .borrow_mut()
            .write(offset, &self.write_buf)
            .map_err(|_| IoError::UnexpectedEof)?;
        self.write_offset += self.write_buf.len() as u32;
        self.write_buf.clear();
        Ok(())
    }
}

impl<F: NorFlash> Write for FlashRetention<F> {
    fn write_all(&mut self, buf: &[u8]) -> Result<(), IoError> {
        self.write_buf.extend_from_slice(buf);
        let ws = self.write_size as usize;
        // Flush complete aligned chunks
        while self.write_buf.len() >= ws {
            let offset = self.base_offset + self.write_offset;
            let chunk_len = (self.write_buf.len() / ws) * ws;
            self.flash
                .borrow_mut()
                .write(offset, &self.write_buf[..chunk_len])
                .map_err(|_| IoError::UnexpectedEof)?;
            self.write_offset += chunk_len as u32;
            self.write_buf.drain(..chunk_len);
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), IoError> {
        self.flush_buf()
    }
}

/// Reader for a byte range from flash.
pub struct FlashReader<'a, F> {
    flash: &'a RefCell<F>,
    offset: u32,
    remaining: u32,
}

impl<F: ReadNorFlash> Read for FlashReader<'_, F> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let max = buf.len().min(self.remaining as usize);
        self.flash
            .borrow_mut()
            .read(self.offset, &mut buf[..max])
            .map_err(|_| IoError::UnexpectedEof)?;
        self.offset += max as u32;
        self.remaining -= max as u32;
        Ok(max)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), IoError> {
        if buf.len() as u32 > self.remaining {
            return Err(IoError::UnexpectedEof);
        }
        self.flash
            .borrow_mut()
            .read(self.offset, buf)
            .map_err(|_| IoError::UnexpectedEof)?;
        self.offset += buf.len() as u32;
        self.remaining -= buf.len() as u32;
        Ok(())
    }
}

impl<F: NorFlash + ReadNorFlash> Retention for FlashRetention<F> {
    type Reader<'a>
        = FlashReader<'a, F>
    where
        F: 'a;

    fn reader(&self, offset: u64, len: u64) -> Result<Self::Reader<'_>, IoError> {
        Ok(FlashReader {
            flash: &self.flash,
            offset: self.base_offset + offset as u32,
            remaining: len as u32,
        })
    }

    fn discard(&mut self) -> Result<(), IoError> {
        self.write_buf.clear();
        let total = self.write_offset;
        if total == 0 {
            return Ok(());
        }
        let start = self.base_offset;
        let end = start + total.div_ceil(self.erase_size) * self.erase_size;
        self.flash
            .borrow_mut()
            .erase(start, end)
            .map_err(|_| IoError::UnexpectedEof)?;
        self.write_offset = 0;
        Ok(())
    }
}
