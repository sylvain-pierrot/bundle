//! Streaming read adapters for deferred retention.

use std::io::{Read, Write};

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
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.reader.read(buf)?;
        if n > 0 {
            self.writer.write_all(&buf[..n])?;
        }
        Ok(n)
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
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.reader.read(buf)?;
        if n > 0 {
            self.buffer.extend_from_slice(&buf[..n]);
        }
        Ok(n)
    }
}

/// Reader that defers retention until explicitly activated.
///
/// Starts in `Capturing` mode (headers buffered in memory).
/// Switches to `Teeing` mode when retention is activated
/// (captured headers flushed to retention, payload streamed).
pub(crate) enum DeferredReader<R, S: Retention> {
    Capturing(CaptureReader<R>),
    Teeing(TeeReader<R, S>),
    Taken,
}

impl<R, S: Retention> DeferredReader<R, S> {
    /// Switch from capturing to teeing.
    ///
    /// Flushes captured header bytes to retention, then wraps
    /// the source in a TeeReader for ongoing payload streaming.
    pub fn activate_retention(&mut self, mut retention: S) -> std::io::Result<()> {
        let old = std::mem::replace(self, DeferredReader::Taken);
        match old {
            DeferredReader::Capturing(capture) => {
                let (source, header_bytes) = capture.into_parts();
                retention.write_all(&header_bytes)?;
                *self = DeferredReader::Teeing(TeeReader::new(source, retention));
                Ok(())
            }
            _ => Err(std::io::Error::other(
                "activate_retention called in wrong state",
            )),
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
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            DeferredReader::Capturing(c) => c.read(buf),
            DeferredReader::Teeing(t) => t.read(buf),
            DeferredReader::Taken => Ok(0),
        }
    }
}
