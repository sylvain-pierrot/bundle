//! Read adapter that copies all bytes to a writer.

use std::io::{Read, Write};

/// A reader that writes every byte read to a secondary writer.
pub(crate) struct TeeReader<R, W> {
    reader: R,
    writer: W,
}

impl<R, W> TeeReader<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }

    pub fn into_parts(self) -> (R, W) {
        (self.reader, self.writer)
    }
}

impl<R: Read, W: Write> Read for TeeReader<R, W> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.reader.read(buf)?;
        if n > 0 {
            self.writer.write_all(&buf[..n])?;
        }
        Ok(n)
    }
}
