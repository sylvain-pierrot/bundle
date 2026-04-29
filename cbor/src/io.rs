//! I/O traits for streaming CBOR.
//!
//! [`CborRead`] and [`CborWrite`] abstract over `std::io` (with `std`
//! feature) and `embedded-io` (without `std`). The streaming codec
//! is written once against these traits.

use crate::Error;

/// Minimal read trait for streaming CBOR decode.
pub trait CborRead {
    fn cbor_read(&mut self, buf: &mut [u8]) -> Result<usize, Error>;
    fn cbor_read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error>;
}

/// Minimal write trait for streaming CBOR encode.
pub trait CborWrite {
    fn cbor_write_all(&mut self, buf: &[u8]) -> Result<(), Error>;
    fn cbor_flush(&mut self) -> Result<(), Error>;
}

#[cfg(feature = "std")]
impl<R: std::io::Read> CborRead for R {
    fn cbor_read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        std::io::Read::read(self, buf).map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                Error::UnexpectedEof
            } else {
                Error::Io(e)
            }
        })
    }

    fn cbor_read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        std::io::Read::read_exact(self, buf).map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                Error::UnexpectedEof
            } else {
                Error::Io(e)
            }
        })
    }
}

#[cfg(feature = "std")]
impl<W: std::io::Write> CborWrite for W {
    fn cbor_write_all(&mut self, buf: &[u8]) -> Result<(), Error> {
        std::io::Write::write_all(self, buf).map_err(Error::Io)
    }

    fn cbor_flush(&mut self) -> Result<(), Error> {
        std::io::Write::flush(self).map_err(Error::Io)
    }
}

#[cfg(not(feature = "std"))]
impl<R: embedded_io::Read> CborRead for R {
    fn cbor_read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        embedded_io::Read::read(self, buf).map_err(|_| Error::IoError)
    }

    fn cbor_read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        embedded_io::Read::read_exact(self, buf).map_err(|e| match e {
            embedded_io::ReadExactError::UnexpectedEof => Error::UnexpectedEof,
            embedded_io::ReadExactError::Other(_) => Error::IoError,
        })
    }
}

#[cfg(not(feature = "std"))]
impl<W: embedded_io::Write> CborWrite for W {
    fn cbor_write_all(&mut self, buf: &[u8]) -> Result<(), Error> {
        embedded_io::Write::write_all(self, buf).map_err(|_| Error::IoError)
    }

    fn cbor_flush(&mut self) -> Result<(), Error> {
        embedded_io::Write::flush(self).map_err(|_| Error::IoError)
    }
}
