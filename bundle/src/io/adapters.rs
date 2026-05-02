//! Streaming read adapters for deferred retention.

use alloc::vec::Vec;

use bundle_io::{Error as IoError, Read, Write};

use crate::retention::Retention;

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
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        let n = self.reader.read(buf)?;
        if n > 0 {
            self.writer.write_all(&buf[..n])?;
        }
        Ok(n)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), IoError> {
        self.reader.read_exact(buf)?;
        self.writer.write_all(buf)?;
        Ok(())
    }
}

/// A reader that captures all bytes read into an internal buffer.
pub(crate) struct CaptureReader<R> {
    reader: R,
    buffer: Vec<u8>,
}

impl<R> CaptureReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: Vec::with_capacity(4096),
        }
    }

    pub fn into_parts(self) -> (R, Vec<u8>) {
        (self.reader, self.buffer)
    }
}

impl<R: Read> Read for CaptureReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        let n = self.reader.read(buf)?;
        if n > 0 {
            self.buffer.extend_from_slice(&buf[..n]);
        }
        Ok(n)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), IoError> {
        self.reader.read_exact(buf)?;
        self.buffer.extend_from_slice(buf);
        Ok(())
    }
}

/// Reader that defers retention until explicitly activated.
pub(crate) enum DeferredReader<R, S: Retention> {
    Capturing(CaptureReader<R>),
    Teeing(TeeReader<R, S>),
    Taken,
}

impl<R, S: Retention> DeferredReader<R, S> {
    /// Activate retention, flushing captured bytes.
    pub fn activate_retention(&mut self, retention: S) -> Result<(), IoError> {
        self.activate_inner(retention, None)
    }

    /// Activate retention with re-encoded headers (mutated version).
    ///
    /// Discards the original captured bytes. Writes the re-encoded
    /// headers to retention, then payload streams through TeeReader.
    pub fn activate_retention_replacing(
        &mut self,
        retention: S,
        encoded_headers: &[u8],
    ) -> Result<(), IoError> {
        self.activate_inner(retention, Some(encoded_headers))
    }

    fn activate_inner(&mut self, mut retention: S, replace: Option<&[u8]>) -> Result<(), IoError> {
        let old = core::mem::replace(self, DeferredReader::Taken);
        match old {
            DeferredReader::Capturing(capture) => {
                let (source, captured) = capture.into_parts();
                retention.write_all(replace.unwrap_or(&captured))?;
                *self = DeferredReader::Teeing(TeeReader::new(source, retention));
                Ok(())
            }
            _ => Err(IoError::UnexpectedEof),
        }
    }

    pub fn into_retention(self) -> Option<S> {
        match self {
            DeferredReader::Teeing(tee) => {
                let (_, retention) = tee.into_parts();
                Some(retention)
            }
            _ => None,
        }
    }
}

impl<R: Read, S: Retention> Read for DeferredReader<R, S> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        match self {
            DeferredReader::Capturing(c) => c.read(buf),
            DeferredReader::Teeing(t) => t.read(buf),
            DeferredReader::Taken => Ok(0),
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), IoError> {
        match self {
            DeferredReader::Capturing(c) => c.read_exact(buf),
            DeferredReader::Teeing(t) => t.read_exact(buf),
            DeferredReader::Taken => Err(IoError::UnexpectedEof),
        }
    }
}
